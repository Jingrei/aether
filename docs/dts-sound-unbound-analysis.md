# How DTS Sound Unbound works (as installed on this PC)

Analyzed package: `DTSInc.DTSSoundUnbound_2026.3.31.0_x64__t5j2fzbtdg37r` (Microsoft Store app).
Method: static inspection of the package manifest, file layout, embedded strings, localized resources, and the
schema of its SQLite databases. Encrypted/obfuscated data files were **not** decrypted, and no DTS code or filter data was
copied into this project.

## Architecture

| Layer | What it is | Evidence |
|---|---|---|
| Renderer | `x64\DtsHrtfEnc.dll`, registered as a Windows **spatial audio encoder** (Media Foundation transform) named "DTS Headphone:X" and "DTS:X Ultra". It takes `MFAudioFormat_Float` in (`00000003-0000-0010-8000-00AA00389B71`) and outputs a vendor format GUID. Windows' spatial sound platform loads it inside `audiodg.exe`. | `AppxManifest.xml`: `windows.mediaCodec` / `audioEncoder` / `DTS.DtsHeadphonesEncoder`, `DTS.DtsSpeakersEncoder` |
| HDMI / decoder | `DtsxHdmiEnc.dll` (DTS:X for home theater bitstream) and `DTSXDecoder.dll` (decoder for DTS content). | Same manifest |
| Control app | .NET Native UWP front end (`DTSSoundUnbound2.dll`). | `build:Metadata` (`ilc.exe`, `.NETCore v5.0`) |
| Settings path | The app writes a "device profile blob" into the audio endpoint's property store. The encoder receives a property-change notification and reloads. | App: `TryWriteDeviceProfileBlobToCapxPropertyStore`, `SetDeviceProfileBlob`. Encoder: `CAPx Property Change Notification received`, `DeviceProfileSettingsBLOB`, `IAudioProcessingObjectNotifications`, `m_StereoBypassMode` |
| Licensing | `DTSDsecProxy.dll`, `LicenseService.*` background tasks (trial and expiry timers). | Manifest extensions |

## Modes ("SAD" / SPAC profiles)

Each listening mode is an encrypted profile in `Data\SAD\*.SPAC.crypt`. The app maps menu options to profile names.

| UI name | Profile file | Notes |
|---|---|---|
| Natural / Spacious (defaults) | `02/04-SPAC-HqHeightAndHgNf_SD{0,1,2}_{Spk,Hp}_Normal_v4_RC2` | `SADMenuOptionNatural`, `SADMenuOptionSpacious` |
| Gaming: Neutral / Balanced / Spacious / Spacious 2 | `2403-Gaming*.SPAC.crypt` | "Partner" modes, unlocked for specific USB headsets (e.g. `VID_03F0&PID_08B7`) |
| Movies: Balanced / Spacious | `2403-Movies*.SPAC.crypt` | Partner modes |
| Entertainment (multichannel) | `04-SPAC-v2-Spatial_Multi_Channel_Entertainment_New` | `ApplyEntertainment2022SAD` |
| Low-power variants | `*-BasicSDContent_*_LowPower_v4_RC2` | Chosen by the Power page |

The name parts encode the rendering features: `Hp` = headphones, `Spk` = speakers, `Xbox`, `HqHeight` = height
virtualization, `Nf` = near-field, `LowPower` / `BasicSDContent` = reduced renderer.

## Power / quality

`PowerSettingsPage` has separate *On battery* / *Plugged in* choices: **Best audio performance / Balanced / Best battery
life** (`CONST_POWERSETTINGS_OPTION_BEST_SOUND / BALANCED / BEST_POWER`). These select the Normal or LowPower profile set.

## Headphone tuning

* Choices: **Bypass**, **Generic Earbuds**, **Generic Over-Ear**, or a specific model (`TryUpdateHeadphoneTuningBypass/Generic/Custom`).
* `offlinedatabase_ui_*.db`: 139 brands and 554 products (types Over-Ear / On-Ear / In-Ear / Ear-buds), plus Bluetooth and USB lookup tables for auto-detection.
* `offlinedatabase_tuning_*.db`: 1,208 `profile_<uuid>_<route>_<44k|48k>.bin` blobs, each 11,552 bytes, obfuscated. Routes are Line-Out, Bluetooth and USB.

## Spatial Sound Editor

`SpatialEditorWidget` plus `m_heightSlider` drive `SpatialSoundPlayer.SpatialEditor` (create, move, destroy): drag a
ball in a 2D area and set its height to hear a positioned test object.

## How Aether reproduces each feature (independent implementation)

| DTS feature | Aether equivalent |
|---|---|
| Headphone:X binaural renderer | Parametric HRTF (spherical head + pinna model, diffuse-field EQ) with image-source room reflections, convolved per virtual speaker of a 7.1.4 bed, plus an FDN late reverb |
| DTS:X Ultra (speakers) | The same binaural render followed by recursive crosstalk cancellation |
| Stereo bypass | **Stereo** mode: no virtualization, enhancements only |
| Natural / Spacious / Gaming / Movies modes | Built-in profiles with the same intent (room size, reflections, speaker angles, height, detail) plus unlimited custom profiles with import/export |
| Power settings | **Quality**: Best sound / Balanced / Battery (filter length, reflection order, height layer, reverb) |
| Headphone tuning | Bypass and generic curves, plus import of AutoEQ / Equalizer APO parametric presets |
| Spatial Sound Editor | Drag-to-position pink-noise test object with a height slider |
| Runs inside Windows' audio engine | Runs as a user-mode app: virtual cable capture (or WASAPI loopback), then process, then the real device. No driver signing needed |
| (not in DTS) | System tray menu, start with Windows, live spectrum and meters |
