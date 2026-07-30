@echo off
cd /d D:\ag
powershell.exe -NoExit -ExecutionPolicy Bypass -File D:\ag\scripts\account-test\start-desktop.ps1 -ProjectRoot D:\ag -RuntimeRoot D:\pomegranate-local-test -PostgresRoot E:\ag-tools\pgsql
