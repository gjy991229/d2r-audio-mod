param(
    [string]$SourceDirectory = '',
    [string]$WorkDirectory = ''
)
$ErrorActionPreference = 'Stop'
$commit = '6bb1882bd00ddbc3729cac5dac0fda81a61e5514'
if (-not $WorkDirectory) {
    $WorkDirectory = Join-Path $PSScriptRoot ('../../target/stormlib-probe-' + [guid]::NewGuid().ToString('N'))
}
$WorkDirectory = [IO.Path]::GetFullPath($WorkDirectory)
if (Test-Path -LiteralPath $WorkDirectory) { throw 'WorkDirectory must not exist; no overwrite is permitted.' }
New-Item -ItemType Directory -Path $WorkDirectory | Out-Null
function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE" }
}
$source = Join-Path $WorkDirectory 'StormLib'
$build = Join-Path $WorkDirectory 'build'
$probeBuild = Join-Path $WorkDirectory 'probe-build'
Invoke-Checked git @('clone', '--depth', '1', '--branch', 'v9.40', 'https://github.com/ladislav-zezula/StormLib.git', $source)
$actual = & git -C $source rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $actual.Trim() -ne $commit) { throw 'StormLib source commit mismatch' }
Invoke-Checked cmake @('-S', $source, '-B', $build, '-A', 'x64', '-DBUILD_SHARED_LIBS=OFF', '-DSTORM_UNICODE=ON', '-DSTORM_USE_BUNDLED_LIBRARIES=ON', '-DSTORM_BUILD_TESTS=OFF')
Invoke-Checked cmake @('--build', $build, '--config', 'Release', '--parallel', '4')
Invoke-Checked cmake @('-S', $PSScriptRoot, '-B', $probeBuild, '-A', 'x64', "-DSTORM_SOURCE=$source", "-DSTORM_LIBRARY=$build/Release/StormLib.lib")
Invoke-Checked cmake @('--build', $probeBuild, '--config', 'Release')
$cases = Join-Path $WorkDirectory 'cases'
$probeArguments = @($cases)
if ($SourceDirectory) { $probeArguments += [IO.Path]::GetFullPath($SourceDirectory) }
Invoke-Checked "$probeBuild/Release/stormlib-probe.exe" $probeArguments
Invoke-Checked rustc @('--edition', '2021', "$PSScriptRoot/ffi_probe.rs", '-L', "native=$build/Release", '-o', "$WorkDirectory/rust-ffi-probe.exe")
Invoke-Checked "$WorkDirectory/rust-ffi-probe.exe" @("$cases/中文 压缩包.mpq")
Write-Output "All checks passed. Artifacts: $WorkDirectory"
