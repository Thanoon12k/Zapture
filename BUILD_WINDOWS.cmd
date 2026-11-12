@echo off
cargo test --locked
if errorlevel 1 exit /b 1
cargo build --release --locked
if errorlevel 1 exit /b 1
if not exist dist mkdir dist
copy /Y target\release\zapture.exe dist\Zapture.exe
copy /Y LICENSE dist\LICENSE
copy /Y THIRD_PARTY.md dist\THIRD_PARTY.md
copy /Y README.md dist\README.md
echo Build ready in dist\Zapture.exe
