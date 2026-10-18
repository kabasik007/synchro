# Synchro · Rust → Android

**Drop a freshly built APK on your Windows PC; receive it directly on Android without Viber, archives, or cloud uploads.**

Synchro is an experimental open-source PC-to-phone file transfer utility. The Windows/Linux sender is **Rust + egui** and the Android 10+ receiver is **native Kotlin**.

## What v0.1 contains

- Drag and drop APKs, ZIPs or other files into the Rust desktop window.
- Configure phone address and secret token once; settings saved in your user's config directory.
- Send each file directly over trusted Wi-Fi or a private Tailscale network.
- Stream uploads to **Download/Synchro/Debug/**, verifying SHA-256 before exposing the file.
- Tap **Install APK** on Android to open the system installer. This requires Android confirmation.

**Status: v0.2 source UI implemented, compile and transfer tests are still needed on your Windows/Android devices.** There are no pre-built EXE or APK release artifacts yet.

## Modern desktop UI (v0.2)

The desktop now uses a custom dark sidebar with **Передача / Пристрій / Налаштування / Про програму**, editable Android pairing, a native multi-file picker, drag-and-drop, a persistent pending queue, transfer progress and a real **Перевірити** connection test. The connectivity check calls authenticated \`GET /health\`, so **both** the Windows app and Android receiver must be updated to v0.2 for the green "Android онлайн" state.

QR pairing and automatically detected device names are **not** included yet: these controls are intentionally not simulated. Use the phone's actual IP address and 32-character pairing token.

**After pulling the new code**, build both programs again, keeping Android's receiver open:

\`\`\`powershell
cd M:\projects\synchro
git pull --ff-only
cd desktop
cargo check
cargo run --release
\`\`\`

Build the corresponding new Android APK in Android Studio or using \`android\gradlew.bat assembleDebug\`. This is a source change and **has not been compile-tested on Windows** in this environment.

## Running the desktop sender on Windows

Requirements: updated Rust stable (edition 2024) and C++ build tools for the Windows MSVC toolchain.

```powershell
git clone https://github.com/kabasik007/synchro.git
cd synchro\desktop
cargo test
cargo run --release
```

Or from repository root, use `scripts/build-windows.ps1`. The executable builds at `desktop/target/release/synchro-desktop.exe`.

## Building Android receiver

Requirements: Android Studio, SDK 36, JDK 17, Android Gradle Plugin 8.13.2, Gradle **8.13** (not Gradle 9.x), Kotlin 2.3.0.

1. Open the repository's `android` directory in Android Studio.
2. In **Settings → Build, Execution, Deployment → Build Tools → Gradle**, choose **Gradle Wrapper** (the project's `gradle-wrapper.properties`, not the local Gradle 9.6 installation). Set **Gradle JDK** to JDK 17.
3. Sync Gradle. The repository pins Gradle 8.13. The Windows `gradlew.bat` downloads the official Gradle 8.13 Wrapper JAR from Gradle's [v8.13.0 GitHub source tag](https://github.com/gradle/gradle/blob/v8.13.0/gradle/wrapper/gradle-wrapper.jar) (with GitHub API fallback), then verifies its SHA-256 against the [Gradle 8.13 release checksum](https://gradle.org/release-checksums/) on first run. The normal Gradle distribution ZIP is then downloaded by the verified wrapper.
4. Run **Build → Build APK(s)**, or use the following from PowerShell:

```powershell
cd synchro\android
.\gradlew.bat --version
.\gradlew.bat assembleDebug
```

5. Install `android/app/build/outputs/apk/debug/app-debug.apk` on a physical Android 10+ device.

The local debug APK is automatically debug-signed by Android tooling, and is not a production-signed release.

## Pair devices and send

1. Open the Android app and tap **Start receiving**.
2. Find the Wi-Fi address shown in the app, e.g. `http://192.168.1.25:8765`, and the 32-character pairing token.
3. Enter the address and token in Synchro Desktop. Drop one or more files into the window.
4. Transfers start automatically when pairing is configured. The files appear in Android's `Download/Synchro/Debug/`.
5. In Synchro Android, choose **Install APK** for an APK or **Open file** for another type.

Android asks for explicit approval before an APK can be installed; it may also require granting **Install unknown apps → Synchro** once. Ordinary third-party Android apps cannot silently install APKs.

## Developer shortcut: direct ADB installation

If your Android phone is already authorized for USB or wireless debugging, simply drag your signed APK onto `scripts/drop-apk-to-install.cmd` in Windows Explorer. The helper invokes `adb install -r` using Android SDK Platform Tools. In this mode, neither the Android receiver app nor transfer configuration is required. Use this only for your own development devices.

## Over the Internet — without your own server

Install [Tailscale](https://tailscale.com/) on Windows and Android, connect both devices to your private tailnet, and use the phone's `100.x.y.z` address as `http://100.x.y.z:8765` in Synchro Desktop. Connections are secured by Tailscale, which may relay traffic when a direct path is not possible.

A custom server, aaPanel, port forwarding, and Cloudflare Tunnel are **not necessary** for this mode. Both devices must be online; the Android receiving service must be started.

**Security:** Never port-forward port 8765 from the public internet. Plain HTTP on a local network is authenticated but not encrypted; use trusted Wi-Fi or an encrypted private network.

## How it works

```text
Windows: Rust + egui           Android: Kotlin receiver
  APK / ZIP drop                   foreground receiver
       |                                   |
  SHA-256 → HTTP POST  ─────────► token validation
     Wi-Fi/Tailscale                 streaming + SHA-256
                                            |
                                  Download/Synchro/Debug
                                            |
                                    Tap Install APK
                                            |
                                    Android system installer
```

See [transfer protocol](docs/PROTOCOL.md) and [roadmap](docs/ROADMAP.md).

## License

MIT · see [LICENSE](LICENSE).
