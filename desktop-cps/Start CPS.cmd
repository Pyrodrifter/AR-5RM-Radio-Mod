@echo off
setlocal
pushd "%~dp0" || exit /b 1

where npm.cmd >nul 2>&1
if errorlevel 1 (
    echo Install Node.js 24 with npm, then open Start CPS.cmd again.
    goto failed
)

if not exist "node_modules\.bin\electron.cmd" (
    echo Installing AR-5RM CPS dependencies...
    call npm.cmd ci
    if errorlevel 1 goto failed
)

call npm.cmd start -- %*
if errorlevel 1 goto failed
popd
exit /b 0

:failed
echo.
echo AR-5RM CPS could not start. See the message above.
pause
popd
exit /b 1
