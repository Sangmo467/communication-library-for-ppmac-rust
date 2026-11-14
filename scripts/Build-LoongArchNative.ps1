[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
# 挂载的 checkout 属于 runner 用户，容器中显式信任这一工作目录。
git config --global --add safe.directory $root
$work = Join-Path $root 'build/loongarch-toolchain'
$sysroot = Join-Path $root 'build/loongarch-sysroot'
New-Item -ItemType Directory -Force -Path $work,$sysroot | Out-Null
$archive = Join-Path $work 'glibc-2.36.tar.xz'
Invoke-WebRequest 'https://ftp.gnu.org/gnu/glibc/glibc-2.36.tar.xz' -OutFile $archive
# 固定 GNU 源码，SHA-256 可与 crosstool-ng 1.26.0 的 glibc/2.36/chksum 交叉核对。
if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne '1c959fea240906226062cb4b1e7ebce71a9f0e3c0836c09e7e3423d434fcfe75') { throw 'glibc source checksum mismatch' }
tar -xf $archive -C $work
if ($LASTEXITCODE -ne 0) { throw 'Cannot extract glibc source' }
$object = Join-Path $work 'objects'
New-Item -ItemType Directory -Force $object | Out-Null
Push-Location $object
try {
    $env:CC = 'loongarch64-linux-gnu-gcc-15'
    $env:CXX = 'loongarch64-linux-gnu-g++-15'
    $env:CFLAGS = '-O2 -std=gnu11'
    & (Join-Path $work 'glibc-2.36/configure') --prefix=/usr --host=loongarch64-linux-gnu --build=x86_64-linux-gnu --enable-kernel=5.19 --disable-werror --with-headers=/usr/loongarch64-linux-gnu/include
    if ($LASTEXITCODE -ne 0) { throw 'glibc sysroot configuration failed' }
    make -j2
    if ($LASTEXITCODE -ne 0) { throw 'glibc sysroot build failed' }
    make install "DESTDIR=$sysroot"
    if ($LASTEXITCODE -ne 0) { throw 'glibc sysroot installation failed' }
} finally { Pop-Location; Remove-Item Env:CC,Env:CXX,Env:CFLAGS -ErrorAction SilentlyContinue }
foreach ($headers in @('linux','asm','asm-generic')) {
    Copy-Item -LiteralPath "/usr/loongarch64-linux-gnu/include/$headers" -Destination (Join-Path $sysroot 'usr/include') -Recurse
}
# 测试使用低版本 libc；libgcc 由工具链提供，不将构建用 glibc 分发给用户。
$libgcc = ((& loongarch64-linux-gnu-gcc-15 -print-file-name=libgcc_s.so.1) -join '').Trim()
New-Item -ItemType Directory -Force (Join-Path $sysroot 'lib64') | Out-Null
Copy-Item -LiteralPath $libgcc -Destination (Join-Path $sysroot 'lib64/libgcc_s.so.1')
$wrapper = Join-Path $work 'loongarch-gcc'
$wrapperText = "#!/bin/sh`nexec loongarch64-linux-gnu-gcc-15 --sysroot=`"$sysroot`" `"`$@`"`n"
[IO.File]::WriteAllText($wrapper,$wrapperText,[Text.UTF8Encoding]::new($false))
chmod +x $wrapper
$env:PMAC_LOONGARCH_SYSROOT = $sysroot
$env:PMAC_LOONGARCH_LINKER = $wrapper
& (Join-Path $PSScriptRoot 'Build-Release.ps1') -Target loongarch64-unknown-linux-gnu
