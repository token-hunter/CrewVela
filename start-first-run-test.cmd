@echo off
setlocal
if not exist "%~dp0src-tauri\target\release\CrewVela.exe" (
  echo Build CrewVela first: npm ci ^&^& npm run tauri -- build --no-bundle
  pause
  exit /b 1
)
start "" "%~dp0src-tauri\target\release\CrewVela.exe" --test-data-dir "%~dp0.test-data"
