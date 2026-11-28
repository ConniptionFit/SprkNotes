# SprkNotes for Android

> **Status: in development.** There is no Android release yet. Nothing on this page installs a finished app today.
> The conversation screen, Settings, the phone microphone, the speech engine and the model download are built; the
> Library, the Light model tier, downloads that finish in the background and a test on a real phone are still to come
> ([ADR-015](adr/015-android.md)). To try it on your own phone, see [Testing on your own phone](#testing-on-your-own-phone).
> An Android build is published only with the owner's approval.

SprkNotes for Android is the same Rust and Tauri app as the Windows version, built for phones (Samsung Galaxy first).
Transcription runs on the phone only. Nothing leaves the device except model and app downloads.

## At a glance
| | |
|---|---|
| Phones | Android 7.0 (API 24) or newer, 64-bit Arm (arm64-v8a). Samsung Galaxy phones from the last several years qualify. |
| App size | 45.5 MB (unsigned arm64 release, measured 2026-11-27 with the speech engine). Android keeps the native libraries uncompressed inside the APK so the phone runs them in place and nothing extra is unpacked at install. CI prints the size on every build. |
| Models | Downloaded separately, never inside the app: Parakeet (about 460 MB download) on phones with 8 GB RAM or more that pass a short self-check, Whisper base.en (154 MB) on the rest. |
| Audio source | The phone's microphone only (built-in, wired or Bluetooth). Android does not let apps capture other apps' call audio, so there is no Me and Others split and no speaker labels. |
| Look | Conversation layout, dark by default, the same font, icons and sound meter as the desktop app. |
| Distribution | Private only, never the public Play Store: pushed by your MDM or as a managed Google Play private app. |

## Installing on a managed phone (planned)
This is how IT will install SprkNotes on company phones once a release exists.

1. **Get the release.** Each Android release on the [Releases page](https://github.com/ConniptionFit/SprkNotes/releases)
   will carry a signed `SprkNotes_<version>_arm64.apk` and a SHA-256 checksum. Check the file before uploading it:
   `Get-FileHash .\SprkNotes_<version>_arm64.apk -Algorithm SHA256` (or `sha256sum` on Linux and macOS).
2. **Add it to your MDM as a private app.** Use one of these, whichever your fleet supports:
   - your MDM's own private app (APK) upload, or
   - a managed Google Play private app published to your organization only, or
   - Samsung Knox Manage on Galaxy phones.

   Whether NinjaOne can push a private APK to Samsung phones is still being confirmed (ADR-015 open item 2).
3. **Grant the microphone permission** in the app's policy so people are not prompted, or let each person allow it on
   first start. SprkNotes will ask for the microphone only; it needs no contacts, storage, location or phone permissions.
4. **Provide the model.** Either let people press **Download** on the main screen (needs https access to the model host
   or your mirror), or push the `.sprkmodel` pack with the MDM so phones never download it. Admin settings such as
   blocking model downloads will follow the same names as the [desktop policy keys](deployment.md#registry-policy),
   delivered as Android managed configuration.
5. **Assign it** to the people or groups who should have it. Updates go out the same way, through the MDM.

## Installing on one phone (planned)
For a pilot, a single signed APK can be installed by hand:
1. Copy `SprkNotes_<version>_arm64.apk` to the phone and check its checksum as above.
2. Open it from **My Files** (Samsung) or **Files**. Android asks once to allow installs from that app: choose
   **Settings**, turn on **Allow from this source**, then go back and choose **Install**.
3. Open SprkNotes, allow the microphone, and press **Download** under **Get the speech model**.
4. Turn **Allow from this source** off again afterwards.

Company phones often block installs from unknown sources; use the managed path above for those.

## First start
1. Open SprkNotes and allow the microphone.
2. If no model was pushed, the main screen shows **Get the speech model**. Press **Download** (about 480 MB, once;
   use Wi-Fi and keep SprkNotes open until it finishes). The download is checked against its checksums exactly as on
   the desktop ([ADR-005](adr/005-model-download.md)). When your admin blocks model downloads, the card says so instead.
3. Press **Start**. Lines appear as people speak.

Planned: the app will read the phone's hardware and run a 3-second self-check, then pick the Full or Light model tier
and the number of threads. Today every phone gets the Full tier (Parakeet).

## Where things live
| What | Where |
|---|---|
| Transcripts (`.md` and a `.jsonl` sidecar) | The app's private storage on the phone; shared only when you choose **Share** |
| Models | The app's private storage; removed when the app is uninstalled |
| Audio | Never stored |

## Remove
Long-press SprkNotes, **Uninstall**, or remove the assignment in your MDM. Uninstalling removes the app, its models and
its private storage. Share or export any transcript you need to keep first.

## Testing on your own phone
There is no published APK, so a test build is a debug APK you build yourself (below) or one the owner has asked to be
made. A debug APK is signed with Android's throwaway debug key, so it installs on any arm64 phone but cannot be updated
by a signed release later: uninstall it first when a real release arrives.

**Install it**
1. Get `app-universal-debug.apk` onto the phone (USB, or download it on the phone).
2. Either open it from **My Files** and allow installs from that app when Android asks (turn it off again afterwards),
   or install over USB with `adb install -r app-universal-debug.apk` (needs **Developer options** and **USB debugging**).
3. Google Play Protect may warn that the app is from an unknown developer. Choose **More details**, then
   **Install anyway**. Company phones often block this; use a personal test phone.

**First run**
1. Open SprkNotes and allow the microphone.
2. Under **Get the speech model**, press **Download** (about 480 MB, once). Use Wi-Fi and keep SprkNotes on screen until
   it finishes: if Android closes the app in the background, the download stops and you press **Download** again.
3. Press **Start** and talk. Lines appear as bubbles with a time and no speaker name.

**What works:** the conversation screen, the microphone and its device menu, Start, Pause, Resume and End, live
lines, the Line wave meter, the session name, Theme in Settings, and transcripts saved to the app's private storage.

**Not built yet:** the Library (past transcripts cannot be listed, opened or shared on the phone yet), the Light model
for phones under 8 GB of RAM (every phone gets Parakeet, so older phones may be slow or run short of memory), the
hardware self-check, downloads that keep going in the background, and admin settings through managed configuration.
A debug build also runs the app's own code unoptimized, so it can feel slower than a release build will.

## Test builds (developers)
Every change to the repository builds an arm64 debug APK in CI (the **Android** workflow) to prove the shared crates
and the app build for phones. CI does not upload or publish it. To build and install one yourself:

1. Install the Android SDK and NDK, Java 17, Node 22 and Rust with the `aarch64-linux-android` target, then the Tauri
   CLI: `cargo install tauri-cli --version '^2' --locked`.
2. Set `ANDROID_HOME` to the SDK and `NDK_HOME` to the NDK folder.
3. Fetch the speech engine libraries (arm64 only, two files) and point the build at them. Without this the build
   downloads the whole sherpa-onnx Android archive and copies extra libraries into the APK.
   ```bash
   bash scripts/fetch-sherpa.sh android-arm64
   export SHERPA_ONNX_LIB_DIR="$(bash scripts/fetch-sherpa.sh --print android-arm64)"
   ```
4. Build:
   ```bash
   (cd app/ui && npm ci)
   cd app
   cargo tauri android build --debug --apk --target aarch64 --ci
   ```
   The Android project is kept in `app/src-tauri/gen/android`, so there is no `init` step. Do not run
   `cargo tauri android init` over it: that would drop the microphone permission. The APK lands in
   `app/src-tauri/gen/android/app/build/outputs/apk/universal/debug/`.
5. On the phone, turn on **Developer options** and **USB debugging**, connect it, and run
   `adb install -r app-universal-debug.apk`. Or run `cargo tauri android dev` to build, install and open it in one step.

A debug APK is about 256 MB because it keeps debug symbols; the release APK is 45.5 MB (both measured in CI on
2026-11-27). Debug builds are for testing only and must not be given to staff.

## More
- [ADR-015](adr/015-android.md): why one shared Rust app, how device tuning works, and what is still open.
- [Install](install.md) and [Deployment](deployment.md) for the Windows and Linux apps.
