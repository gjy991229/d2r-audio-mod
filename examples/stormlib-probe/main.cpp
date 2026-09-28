// Isolated integration probe, not the production MPQ converter.
// Writes only newly created output roots. Optional input directories are read-only.
#include <StormLib.h>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <map>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>
namespace fs = std::filesystem;

void require(bool ok, const std::string& what) {
    if (!ok) throw std::runtime_error(what + " (error=" + std::to_string(GetLastError()) + ")");
}
struct Archive {
    HANDLE h = nullptr;
    ~Archive() { if (h) SFileCloseArchive(h); }
    void close() { require(SFileCloseArchive(h), "close archive"); h = nullptr; }
};
std::string read(const fs::path& p) {
    const fs::path long_path(std::wstring(L"\\\\?\\") + fs::absolute(p).wstring());
    std::ifstream f(long_path, std::ios::binary);
    require(bool(f), "read file");
    return {std::istreambuf_iterator<char>(f), std::istreambuf_iterator<char>()};
}
void write(const fs::path& p, const std::string& bytes) {
    fs::create_directories(p.parent_path());
    std::ofstream f(p, std::ios::binary);
    f.write(bytes.data(), bytes.size());
    require(bool(f), "write fixture");
}
using Files = std::map<std::string, fs::path>;
Files collect(const fs::path& root) {
    Files files;
    for (auto& e : fs::recursive_directory_iterator(root)) {
        require(!fs::is_symlink(e.symlink_status()), "links unsupported in probe input");
        if (e.is_regular_file()) {
            auto rel = e.path().lexically_relative(root).generic_u8string();
            files.emplace(rel, e.path());
        }
    }
    require(!files.empty(), "input is empty");
    return files;
}
// Comparison-only adapter for legacy GUI percent/Latin-1 output names.
// It does not guess an archive encoding: it reverses each reference name and
// accepts it only if the resulting bytes exactly equal the original MPQ name.
std::string legacy_gui_bytes(const std::string& utf8) {
    auto wide = fs::u8path(utf8).generic_wstring();
    std::string bytes;
    const auto hex = [](wchar_t c) -> int {
        if (c >= L'0' && c <= L'9') return c - L'0';
        if (c >= L'A' && c <= L'F') return c - L'A' + 10;
        if (c >= L'a' && c <= L'f') return c - L'a' + 10;
        return -1;
    };
    for (size_t i = 0; i < wide.size(); ++i) {
        if (wide[i] == L'%' && i + 2 < wide.size() && hex(wide[i+1]) >= 0 && hex(wide[i+2]) >= 0) {
            bytes.push_back(static_cast<char>(hex(wide[i+1]) * 16 + hex(wide[i+2])));
            i += 2;
        } else {
            if (wide[i] > 255) return {};
            bytes.push_back(static_cast<char>(wide[i]));
        }
    }
    return bytes;
}
void pack(const Files& files, const fs::path& archive, bool listfile, DWORD compression, bool encrypted) {
    Archive a;
    SFILE_CREATE_MPQ info{};
    info.cbSize = sizeof(info);
    info.dwMpqVersion = MPQ_FORMAT_VERSION_1;
    info.dwFileFlags1 = listfile ? MPQ_FILE_DEFAULT_INTERNAL : 0;
    info.dwSectorSize = 4096;
    info.dwMaxFileCount = static_cast<DWORD>(files.size() + 16);
    require(SFileCreateArchive2(archive.c_str(), &info, &a.h), "create archive");
    for (const auto& [rel, source] : files) {
        auto name = rel;
        for (auto& c : name) if (c == '/') c = '\\';
        require(SFileAddFileEx(a.h, source.c_str(), name.c_str(),
            MPQ_FILE_COMPRESS | (encrypted ? MPQ_FILE_ENCRYPTED : 0),
            compression, compression), "add " + rel);
    }
    a.close();
}
void roundtrip(const Files& files, const fs::path& archive, const fs::path& destination, bool include_internal = false) {
    Archive a;
    require(SFileOpenArchive(archive.c_str(), 0, MPQ_OPEN_READ_ONLY, &a.h), "open archive");
    SFILE_FIND_DATA item{};
    HANDLE search = SFileFindFirstFile(a.h, "*", &item, nullptr);
    require(search != nullptr && search != INVALID_HANDLE_VALUE, "enumerate");
    size_t count = 0;
    size_t mapped = 0;
    std::set<std::string> seen;
    unsigned long long total = 0;
    try {
        do {
            std::string rel = item.cFileName;
            if (!include_internal && (rel == "(listfile)" || rel == "(attributes)" || rel == "(signature)")) continue;
            for (auto& c : rel) if (c == '\\') c = '/';
            // Only extract paths present in our known input inventory.
            auto expected = files.find(rel);
            if (expected == files.end() && include_internal) {
                for (auto candidate = files.begin(); candidate != files.end(); ++candidate) {
                    if (legacy_gui_bytes(candidate->first) == rel) {
                        require(expected == files.end(), "ambiguous reference path mapping");
                        expected = candidate;
                    }
                }
                if (expected != files.end()) {
                    ++mapped;
                    std::cout << "GUI_NAME_MAPPING " << expected->first << '\n';
                }
            }
            require(expected != files.end(), "unknown path " + rel);
            require(seen.insert(expected->first).second, "duplicate output path");
            auto target = destination / fs::u8path(expected->first);
            fs::create_directories(target.parent_path());
            auto long_target = std::wstring(L"\\\\?\\") + fs::absolute(target).wstring();
            require(SFileExtractFile(a.h, item.cFileName, long_target.c_str(), SFILE_OPEN_FROM_MPQ), "extract " + rel);
            require(read(target) == read(expected->second), "byte mismatch " + rel);
            ++count;
            total += item.dwFileSize;
        } while (SFileFindNextFile(search, &item));
        require(GetLastError() == ERROR_NO_MORE_FILES, "enumeration termination");
    } catch (...) { SFileFindClose(search); throw; }
    SFileFindClose(search);
    require(count == files.size(), "incomplete enumeration");
    std::cout << "PASS roundtrip files=" << count << " bytes=" << total << " gui_name_mappings=" << mapped << '\n';
}
void no_listfile(const Files& files, const fs::path& archive) {
    pack(files, archive, false, MPQ_COMPRESSION_ZLIB, false);
    Archive a;
    require(SFileOpenArchive(archive.c_str(), 0, MPQ_OPEN_READ_ONLY, &a.h), "open no-listfile");
    SFILE_FIND_DATA item{};
    HANDLE search = SFileFindFirstFile(a.h, "*", &item, nullptr);
    size_t unresolved = 0;
    require(search != nullptr && search != INVALID_HANDLE_VALUE, "enumerate no-listfile");
    do {
        std::string rel = item.cFileName;
        for (auto& c : rel) if (c == '\\') c = '/';
        if (files.find(rel) == files.end()) ++unresolved;
        std::cout << "NO_LISTFILE name=" << rel << " block=" << item.dwBlockIndex << '\n';
    } while (SFileFindNextFile(search, &item));
    auto error = GetLastError();
    SFileFindClose(search);
    require(error == ERROR_NO_MORE_FILES && unresolved == files.size(), "missing names detection");
    std::cout << "PASS missing names detected; conversion must be rejected\n";
}
int wmain(int argc, wchar_t** argv) {
    try {
        if (argc == 5 && std::wstring(argv[1]) == L"--compare") {
            // The reference is used only for output path allowlisting and byte comparison.
            // It is never supplied to StormLib as an external listfile.
            fs::path output = argv[4];
            require(!fs::exists(output), "output root must not exist");
            fs::create_directories(output);
            roundtrip(collect(fs::path(argv[3])), fs::path(argv[2]), output, true);
            std::cout << "PASS original MPQ matches GUI reference (including internal entries)\n";
            return 0;
        }
        require(argc == 2 || argc == 3, "usage: stormlib-probe NEW_OUTPUT_ROOT [READ_ONLY_MOD_DIRECTORY]");
        fs::path root = argv[1];
        require(!fs::exists(root), "output root must not exist");
        fs::create_directories(root);
        auto input = root / L"中文 源文件";
        write(input / "modinfo.json", "{\"name\":\"Probe\",\"savepath\":\"Probe/\"}");
        write(input / "data/global/excel/probe.txt", std::string(100000, 'A') + "\r\nend\r\n");
        write(input / "data/local/empty.bin", "");
        std::string binary;
        for (int i = 0; i < 65536; ++i) binary.push_back(static_cast<char>(i));
        write(input / "data/hd/probe.bin", binary);
        auto files = collect(input);
        std::cout << "StormLib " << STORMLIB_VERSION_STRING << "\n";
        pack(files, root / L"中文 压缩包.mpq", true, MPQ_COMPRESSION_ZLIB, false);
        roundtrip(files, root / L"中文 压缩包.mpq", root / L"解压目录/Probe/Probe.mpq");
        pack(files, root / "encrypted-bzip2.mpq", true, MPQ_COMPRESSION_BZIP2, true);
        roundtrip(files, root / "encrypted-bzip2.mpq", root / "encrypted-output/Probe/Probe.mpq");
        no_listfile(files, root / "no-listfile.mpq");
        if (argc == 3) {
            auto real = collect(fs::path(argv[2]));
            pack(real, root / "real-directory-repacked.mpq", true, MPQ_COMPRESSION_ZLIB, false);
            roundtrip(real, root / "real-directory-repacked.mpq", root / "real-repacked-output");
        }
        std::cout << "ALL PROBES PASSED\n";
        return 0;
    } catch (const std::exception& e) {
        std::cerr << "FAIL " << e.what() << '\n';
        return 1;
    }
}
