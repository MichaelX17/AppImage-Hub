# AppImage metadata

These files define AppImage Hub's own application metadata when it is packaged
as an AppImage.

## Required AppDir layout

```text
AppDir/
├── AppRun
├── .DirIcon -> appimage-hub.png
├── io.github.michaelx17.AppImageHub.desktop
├── appimage-hub.png
├── .appimage-hub.json
└── usr/
    ├── bin/
    │   └── appimage-manager
    └── share/
        ├── icons/hicolor/512x512/apps/appimage-hub.png
        └── metainfo/io.github.michaelx17.AppImageHub.metainfo.xml
```

## Files

| Source file | Destination inside AppImage |
| --- | --- |
| `io.github.michaelx17.AppImageHub.desktop` | `/io.github.michaelx17.AppImageHub.desktop` |
| `io.github.michaelx17.AppImageHub.appdata.xml` | `/usr/share/metainfo/io.github.michaelx17.AppImageHub.appdata.xml` |
| `.appimage-hub.json` | `/.appimage-hub.json` |
| `../assets/logo.png` | `/appimage-hub.png` and `/usr/share/icons/hicolor/512x512/apps/appimage-hub.png` |

`Exec=appimage-manager %F` assumes the packaged executable is named
`appimage-manager`.
