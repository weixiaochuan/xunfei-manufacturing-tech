# Account Server file storage

`backup-user-files.ps1` creates a non-overwriting timestamp copy and a privacy-safe SHA-256 manifest. `verify-user-files-backup.ps1` verifies every copied file. Paths are parameters so the same workflow can later be translated to `/srv/pomegranate/user-files` on Linux.

Example:

```powershell
.\scripts\account-storage\backup-user-files.ps1 -Source '<current USER_FILES_ROOT>'
.\scripts\account-storage\verify-user-files-backup.ps1 -BackupPath '<reported backup path>' -ManifestPath '<reported manifest path>'
```

The scripts never delete the source or an older backup.
