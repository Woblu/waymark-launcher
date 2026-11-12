# Releasing Waymark Launcher

## The update signing key

Updates are signed so the app only installs ones that really came from this project. The public half is in `apps/desktop/src-tauri/tauri.conf.json`, and that's safe to publish. The private key has no password, so whoever has the file can sign an update every installed copy would trust.

Keep exactly two copies of it:

1. The `TAURI_SIGNING_PRIVATE_KEY` secret in GitHub, which the release workflow signs with.
2. A backup in a password manager or another encrypted place you control.

Releases are built by GitHub Actions, so the key doesn't need to be on your computer. Once both copies exist, delete `%USERPROFILE%\.waymark\updater.key`. If the key is lost, existing installs can never update again.

## Building on your own computer

You only need this to test a release build. Copy the key from your backup, then run the lines below. The first one reads the key from the clipboard, so it never lands in PowerShell's history. Afterwards, close the terminal and clear the clipboard (Windows key + V, then Clear all).

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = (Get-Clipboard -Raw).Trim()
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
$env:WAYMARK_CURSEFORGE_API_KEY = '<your key>'
cd apps/desktop
npm run tauri build
```

The installer and its `.sig` file land in `target/release/bundle/nsis/`.

## GitHub setup

The repository is `Woblu/waymark-launcher`. Its Actions secrets are `TAURI_SIGNING_PRIVATE_KEY` and `WAYMARK_CURSEFORGE_API_KEY`. `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` stays unset because the key has no password.

The app checks `https://waymark-launcher.vercel.app/updates/latest.json`, and `apps/site/vercel.json` redirects that to the newest published GitHub release. Using our own address means the repository can move without breaking installed copies. If it moves, change the redirect and deploy the site. Until a release is published, the redirect ends in a 404, which the app treats as no update.

## How updates reach people

The app checks 20 seconds after it starts and every 6 hours after that, unless the player has turned off **Keep Waymark Launcher up to date** in Settings. A newer version downloads in the background, and its signature is checked before it's kept and again before it installs. It then installs silently when the app closes, or straight away from **Restart to update**.

An update never installs while a game or a hosted server is running, because the installer has to stop the app, and the app is what watches the game. All of this is in `apps/desktop/src-tauri/src/updates.rs`. The window can only ask for an install, and has no access to the updater itself.

## Making a release

1. Set the version in the root `Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/package.json` and the two `version` lines at the top of `apps/desktop/package-lock.json`, then run `cargo update --workspace --offline`. It has to be higher than the version people have, or the app won't offer it.
2. In `CHANGELOG.md`, give the Unreleased section the new version and today's date.
3. Commit that as "Release 0.6.0" and push a tag like `v0.6.0`. GitHub builds Windows, macOS (Apple silicon and Intel) and Linux, and creates a draft release.
4. Paste the version's changelog section into the draft as its notes, as described below.
5. Test the draft, then publish it. Drafts are invisible to the app, and publishing is the moment every installed copy starts updating, so a bad release reaches people within hours.

### How often

Fewer, larger releases are easier on players and on anyone reviewing the project. Collect changes for a week or two, ship anything big as a beta first, and give the beta a few days. A fix for something broken can go out as soon as it's tested.

### Release notes

The notes are the version's section of `CHANGELOG.md`, plus two lines at the end:

```markdown
### Added
- One line for each thing a player can now do.

### Changed
- One line for each thing that works differently, and why if it isn't obvious.

### Fixed
- One line for each bug, described the way a player saw it.

### Known problems
- Anything a player should know before updating.

Every file in this release is listed in `SHA256SUMS.txt`, so you can check a download.

Windows warns about an unknown publisher, because the installer isn't code-signed. Choose More info, then Run anyway.
```

Leave out headings with nothing under them. Write plain sentences: no bold lead-ins, nothing about what a future version might do, and nothing you haven't checked.

### Betas first

Ship anything big as a beta before it reaches everyone. Tag it with a hyphen, like `v1.0.0-beta.1`, and set the same version in `tauri.conf.json` and `Cargo.toml`. The workflow marks it as a pre-release. Players who turned on **Get beta versions** in Settings pick it up through the GitHub releases list (`RELEASES_API` in `brand.rs`); everyone else follows `/updates/latest.json`, which GitHub points at the newest full release and never at a pre-release.

When a beta has been out for a few days without trouble, tag the full version (`v1.0.0`). Beta players move to it too, since it's higher than any `1.0.0-beta`.

Every release also gets a `SHA256SUMS.txt` listing every file, from the `checksums` job, for anyone checking a download by hand.

If BetaCraft has added builds or LegacyFix has a new release, run `node tools/old-versions.mjs <cache folder>` first, set the `LEGACYFIX` version at the top if it changed, and start the new builds before shipping. It rebuilds `crates/waymark-core/data/old-versions.json` and fetches one file at a time, so keep its cache folder between runs.

The site's Download button needs no change for a new version. `/download` in `apps/site/vercel.json` points at `releases/latest/download/Waymark-Launcher_x64-setup.exe`, and the release workflow adds a copy of the Windows installer under that name to every release, so the button follows whichever release was published last. It also adds one as `Waymark_x64-setup.exe`, the name from before the rename, for links that are already out there. Deploy the site only when its pages change. To stop a bad release, publish a fixed one with a higher version. Deleting the release only stops new downloads.

## The rename

The app was called Waymark until 0.3.7. Tauri's Windows installer keys its folder, its Installed apps entry and its shortcuts to `productName`, so `apps/desktop/src-tauri/windows/installer-hooks.nsh` takes over an install made under the old name: the update goes into the same folder, and the entry and shortcuts move to the new name. Without it, an update would install a second copy, and the old Start menu shortcut would keep opening the old version. The Linux package says it replaces, conflicts with and provides `waymark` for the same reason. The data folder (`DATA_DIR_NAME`) and the keychain service stay as they were, so nobody loses instances or their sign-in. Keep all of this for as long as anyone might update from 0.3.6 or older.

## Signing the installers

The update key above isn't code signing. Without code signing, Windows SmartScreen warns people and macOS blocks the app until they allow it in Privacy & Security. Linux needs no signing.

macOS builds are ad-hoc signed for now (`signingIdentity` is `-`), because Apple silicon won't run unsigned apps from the internet at all. An Apple certificate replaces that.

### Windows: Azure Artifact Signing

This runs inside the build through `bundle.windows.signCommand`, so the app and installer are signed before the update signature is made.

**Who can use it:** individuals in the United States or Canada, and organizations in the United States, Canada, the EU, the UK, Australia, New Zealand, Japan, South Korea, Singapore, Switzerland, Norway and Israel. Trial and free Azure subscriptions don't work.

**Cost:** $9.99 a month for 5,000 signatures. Billing starts when the account is created and isn't prorated.

**What people see:** your verified legal name as the publisher, plus city, state and country. Your street address and email aren't included. Custom names aren't supported.

1. In the Azure portal, add a pay-as-you-go subscription. For an individual, the billing account's type must be Individual, and its legal name and address must match your government ID.
2. Under Subscriptions, Resource providers, register `Microsoft.CodeSigning`.
3. Search for Artifact Signing Accounts and create one on the Basic plan. Note the account name and its endpoint, such as `https://wus2.codesigning.azure.net`.
4. Give yourself the **Artifact Signing Identity Verifier** role on the account, then open Identity validations and create a new identity. Validation takes 1 to 20 business days.
5. Under Certificate profiles, create a Public Trust profile from that identity.
6. In Microsoft Entra, create a new app registration with a client secret just for signing. Don't reuse the Minecraft sign-in app. Give it the **Artifact Signing Certificate Profile Signer** role.
7. Add `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` and `AZURE_TENANT_ID` as Actions secrets.
8. Set `signCommand` for CI and install `artifact-signing-cli` in the Windows release job:

```json
"signCommand": "artifact-signing-cli -e <endpoint> -a <account> -c <profile> -d \"Waymark Launcher\" %1"
```

Certificates last three days, and the signature's timestamp keeps it valid after that. Signing doesn't remove SmartScreen warnings straight away, because reputation builds as people download the installer.

### Windows, free: SignPath Foundation

The only free route for a directly downloaded installer. Waymark Launcher's public source and GPL license already meet the license terms. Still needed:

- A published release in the form you want signed, plus a verifiable reputation. Approval is at their discretion and no review time is published, so it can be refused.
- A code signing policy page on the website, with the wording and team roles from [their terms](https://signpath.org/terms), and a privacy statement.
- MFA on GitHub and SignPath, and manual approval of every release.

The signature names SignPath Foundation as publisher, not you. Signing happens on an uploaded artifact after the build, so the release workflow would have to make the update signature and `latest.json` itself.

The Microsoft Store isn't a way around this. Individual registration is free and the Store signs MSIX packages for free, but Tauri only makes EXE and MSI installers, and Tauri's docs say those must be code signed already.

### macOS: Apple Developer ID

1. Enroll at [developer.apple.com/programs/enroll](https://developer.apple.com/programs/enroll/) as an individual. It costs $99 a year and needs two-factor authentication. Organizations also need a D-U-N-S number, a legal entity, a public website and a domain email.
2. As Account Holder, create a **Developer ID Application** certificate. That's the type for apps outside the App Store. Don't use Apple Development, even though Tauri's sample workflow greps for it.
3. Export it from Keychain Access as a `.p12` with a password, then encode it:

```bash
openssl base64 -A -in certificate.p12 -out certificate-base64.txt
```

Without a Mac, make the key and request with OpenSSL, upload the request to Apple, then build the `.p12` from Apple's certificate. Apple doesn't document this route, so check it on the first signed build:

```bash
openssl req -new -newkey rsa:2048 -nodes -keyout developer-id.key -out developer-id.csr -subj "/CN=<your name>"
openssl x509 -inform DER -in developerID_application.cer -out developer-id.pem
openssl pkcs12 -export -legacy -inkey developer-id.key -in developer-id.pem -out certificate.p12
```

4. At [account.apple.com](https://account.apple.com), create an app-specific password for notarization.
5. Add these Actions secrets:
   - `APPLE_CERTIFICATE`: the contents of `certificate-base64.txt`
   - `APPLE_CERTIFICATE_PASSWORD`: the `.p12` password
   - `APPLE_SIGNING_IDENTITY`: `Developer ID Application: <your name> (<team id>)`
   - `APPLE_ID`: your Apple account email
   - `APPLE_PASSWORD`: the app-specific password
   - `APPLE_TEAM_ID`: from your membership page
6. Pass them to `tauri-action` in `release.yml`. Only add them once they exist, because GitHub turns a missing secret into an empty value and that would override the ad-hoc identity.
