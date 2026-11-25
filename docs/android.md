# SprkNotes for Android

> **Status: in development.** There is no Android release yet. Nothing on this page installs a finished app today:
> the phone screens, the phone microphone and the speech engine are still being built ([ADR-015](adr/015-android.md)).
> Test builds for developers are covered in [Test builds](#test-builds-developers). An Android build is published
> only with the owner's approval.

SprkNotes for Android is the same Rust and Tauri app as the Windows version, built for phones (Samsung Galaxy first).
Transcription runs on the phone only. Nothing leaves the device except model and app downloads.

## At a glance
| | |
|---|---|
| Phones | Android 7.0 (API 24) or newer, 64-bit Arm (arm64-v8a). Samsung Galaxy phones from the last several years qualify. |
| App size | About 13.5 MB today (measured 2026-11-25, before the speech engine). Expected 25 to 40 MB once the engine is in. |
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
4. **Provide the model.** Either let people press **Download model** in Settings (needs https access to the model host
   or your mirror), or push the `.sprkmodel` pack with the MDM so phones never download it. Admin settings such as
   blocking model downloads will follow the same names as the [desktop policy keys](deployment.md#registry-policy),
   delivered as Android managed configuration.
5. **Assign it** to the people or groups who should have it. Updates go out the same way, through the MDM.

## Installing on one phone (planned)
For a pilot, a single signed APK can be installed by hand:
1. Copy `SprkNotes_<version>_arm64.apk` to the phone and check its checksum as above.
2. Open it from **My Files** (Samsung) or **Files**. Android asks once to allow installs from that app: choose
   **Settings**, turn on **Allow from this source**, then go back and choose **Install**.
3. Open SprkNotes, allow the microphone, and press **Download model** in Settings.
4. Turn **Allow from this source** off again afterwards.

Company phones often block installs from unknown sources; use the managed path above for those.

## First start
1. Open SprkNotes and allow the microphone.
2. The app reads the phone's hardware and runs a 3-second self-check, then picks the Full or Light model tier and the
   number of threads. Settings shows the result as **Tuned for this phone**, with **Re-check** to run it again.
3. Press **Download model** if no model was pushed, then press **Start**. Lines appear as people speak.

## Where things live
| What | Where |
|---|---|
| Transcripts (`.md` and a `.jsonl` sidecar) | The app's private storage on the phone; shared only when you choose **Share** |
| Models | The app's private storage; removed when the app is uninstalled |
| Audio | Never stored |

## Remove
Long-press SprkNotes, **Uninstall**, or remove the assignment in your MDM. Uninstalling removes the app, its models and
its private storage. Share or export any transcript you need to keep first.

## Test builds (developers)
Every change to the repository builds an arm64 debug APK in CI (the **Android** workflow) to prove the shared crates
and the app shell build for phones. CI does not upload or publish it. To build and install one yourself:

1. Install the Android SDK and NDK, Java 17, Node 22 and Rust with the `aarch64-linux-android` target, then the Tauri
   CLI: `cargo install tauri-cli --version '^2' --locked`.
2. Set `ANDROID_HOME` to the SDK and `NDK_HOME` to the NDK folder.
3. Build:
   ```bash
   (cd app/ui && npm ci)
   cd app
   cargo tauri android build --debug --apk --target aarch64 --ci
   ```
   The Android project is kept in `app/src-tauri/gen/android`, so there is no `init` step. Do not run
   `cargo tauri android init` over it: that would drop the microphone permission. The APK lands in
   `app/src-tauri/gen/android/app/build/outputs/apk/universal/debug/`.
4. On the phone, turn on **Developer options** and **USB debugging**, connect it, and run
   `adb install -r app-universal-debug.apk`. Or run `cargo tauri android dev` to build, install and open it in one step.

A debug build is about 150 MB because it keeps debug symbols; release builds are about a tenth of that. Debug builds
are for testing only and must not be given to staff.

## More
- [ADR-015](adr/015-android.md): why one shared Rust app, how device tuning works, and what is still open.
- [Install](install.md) and [Deployment](deployment.md) for the Windows and Linux apps.
