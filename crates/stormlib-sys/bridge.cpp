#include <StormLib.h>
extern "C" bool d2r_mpq_file_count(HANDLE archive, DWORD* count) {
    return SFileGetFileInfo(archive, SFileMpqNumberOfFiles, count, sizeof(*count), nullptr);
}
// Deterministic fixtures for Rust integration tests; not used by the CLI.
extern "C" bool d2r_mpq_test_fixture(const wchar_t* path, bool listfile, unsigned variant) {
    SFILE_CREATE_MPQ info = {};
    info.cbSize = sizeof(info);
    info.dwMpqVersion = MPQ_FORMAT_VERSION_1;
    info.dwFileFlags1 = listfile ? MPQ_FILE_DEFAULT_INTERNAL : 0;
    info.dwSectorSize = 4096;
    info.dwMaxFileCount = 16;
    HANDLE archive = nullptr;
    if (!SFileCreateArchive2(path, &info, &archive)) return false;
    const char* names[] = {"modinfo.json", "data\\probe.bin", "data\\empty.bin"};
    if (variant == 1) names[1] = "..\\outside.bin";
    if (variant == 2) names[0] = "wrapper\\modinfo.json";
    if (variant == 3) names[2] = "DATA\\PROBE.bin";
    if (variant == 4) names[2] = "data\\x-\xCE\xDE\xBF\xEC\xBD\xA8\xB7\xBF.json";
    const char* contents[] = {"{\"name\":\"mini\",\"savepath\":\"mini/\"}", "test payload", ""};
    bool ok = true;
    for (unsigned i = 0; i < 3 && ok; ++i) {
        HANDLE file = nullptr;
        DWORD size = static_cast<DWORD>(strlen(contents[i]));
        ok = SFileCreateFile(archive, names[i], 0, size, variant == 5 ? 0x409 : 0, MPQ_FILE_COMPRESS, &file);
        if (ok) {
            bool written = size == 0 || SFileWriteFile(file, contents[i], size, MPQ_COMPRESSION_ZLIB);
            bool finished = SFileFinishFile(file);
            ok = written && finished;
        }
    }
    bool closed = SFileCloseArchive(archive);
    return ok && closed;
}
