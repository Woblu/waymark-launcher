# Releasing Waymark

## The update signing key

Updates are signed so the app only installs ones that really came from you. The private key is at `%USERPROFILE%\.waymark\updater.key`, outside this repository on purpose.

**Keep a backup somewhere safe.** If it's lost, existing installs can never update again. If it leaks, someone could ship an update that installed copies would trust.

The public half is in `apps/desktop/src-tauri/tauri.conf.json`, and that's safe to publish.

## Building on your own computer

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw "$env:USERPROFILE\.waymark\updater.key"
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

The app checks 20 seconds after it starts and every 6 hours after that, unless the player has turned off **Keep Waymark up to date** in Settings. A newer version downloads in the background, and its signature is checked before it's kept and again before it installs. It then installs silently when the app closes, or straight away from **Restart to update**.

An update never installs while a game or a hosted server is running, because the installer has to stop the app, and the app is what watches the game. All of this is in `apps/desktop/src-tauri/src/updates.rs`. The window can only ask for an install, and has no access to the updater itself.

## Making a release

1. Set the version in `apps/desktop/src-tauri/tauri.conf.json` and the root `Cargo.toml`. It has to be higher than the version people have, or the app won't offer it.
2. Push a tag like `v0.2.0`. GitHub builds Windows, macOS (Apple silicon and Intel) and Linux, and creates a draft release.
3. Test the draft, then publish it. Drafts are invisible to the app, and publishing is the moment every installed copy starts updating, so a bad release reaches people within hours.

After publishing, point `/download` in `apps/site/vercel.json` at the new installer and deploy the site. To stop a bad release, publish a fixed one with a higher version. Deleting the release only stops new downloads.

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
"signCommand": "artifact-signing-cli -e <endpoint> -a <account> -c <profile> -d Waymark %1"
```

Certificates last three days, and the signature's timestamp keeps it valid after that. Signing doesn't remove SmartScreen warnings straight away, because reputation builds as people download the installer.

### Windows, free: SignPath Foundation

The only free route for a directly downloaded installer. Waymark's public source and GPL license already meet the license terms. Still needed:

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
