$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
if (-not (Test-Path '.venv\Scripts\python.exe')) {
    python -m venv .venv
    if ($LASTEXITCODE -ne 0) { throw 'Install Python 3 with tkinter, then retry.' }
}
& .venv\Scripts\python.exe -m pip install -r requirements.txt
if ($LASTEXITCODE -ne 0) { throw 'Dependency installation failed.' }
& .venv\Scripts\python.exe -m PyInstaller --noconfirm --clean --onefile --windowed --name DeviceCompanion app.py
if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
Copy-Item -LiteralPath '.\dist\DeviceCompanion.exe' -Destination '.\DeviceCompanion.exe' -Force
Write-Host 'Ready: DeviceCompanion.exe (also in dist)'
