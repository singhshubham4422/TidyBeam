# Publishing TidyBeam to the Microsoft Store

**Store listing:** <https://apps.microsoft.com/detail/9NXSC9HV5S12> (Store ID `9NXSC9HV5S12`, package family `SwiftLabs.TidyBeam_eha7verb3j2br`). The name is reserved and the identity variables are already set in GitHub; the remaining steps are the first manual submission and API access.

`.github/workflows/store.yml` turns each **stable** GitHub release into an MSIX bundle (x64 + ARM64) and submits it to the Microsoft Store. The Store signs the package and updates installed copies automatically, so Store users always get the latest version without a code-signing certificate.

Pre-releases (tags such as `v0.3.0-beta.1`) are skipped. You can also run the workflow by hand from **Actions → Microsoft Store → Run workflow** with any tag.

## One-time setup

### 1. Partner Center

1. Register a Microsoft Store developer account at <https://partner.microsoft.com/dashboard/registration> (one-time fee for individuals).
2. In **Apps and games → New product → MSIX or PWA app**, reserve the name **TidyBeam**.
3. Open **Product management → Product identity** and note:
   - **Package/Identity/Name** (for example `12345Shubham.TidyBeam`)
   - **Package/Identity/Publisher** (starts with `CN=`)
   - **Package/Properties/PublisherDisplayName**
   - **Store ID** (12 characters, for example `9NXXXXXXXXXX`)
4. Complete the first submission once by hand in Partner Center: pricing (Free), properties (category Utilities & tools), age rating, store listing text and screenshots. Upload the `msixbundle` artifact from a manual run of the workflow (`submit` unchecked) as the package. Microsoft asks you to justify the `runFullTrust` capability: “TidyBeam is a desktop file manager that needs standard access to the user's files and folders.”

### 2. API access for automatic submissions

1. In Partner Center, open **Account settings → User management → Microsoft Entra applications** and create (or add) an application with the **Manager** role.
2. Create a client secret (key) for it.
3. Note the **Tenant ID**, **Client ID**, **Client secret** and your **Seller ID** (Account settings → Legal info → Developer).

### 3. GitHub settings

In the repository, open **Settings → Secrets and variables → Actions**.

**Variables**

| Name | Value |
|---|---|
| `MSSTORE_IDENTITY_NAME` | Package/Identity/Name |
| `MSSTORE_PUBLISHER` | Package/Identity/Publisher (`CN=...`) |
| `MSSTORE_PUBLISHER_DISPLAY_NAME` | PublisherDisplayName |
| `MSSTORE_PRODUCT_ID` | Store ID |

**Secrets**

| Name | Value |
|---|---|
| `MSSTORE_TENANT_ID` | Tenant ID |
| `MSSTORE_SELLER_ID` | Seller ID |
| `MSSTORE_CLIENT_ID` | Client ID |
| `MSSTORE_CLIENT_SECRET` | Client secret |

Without the secrets, the workflow still builds the bundle and attaches it as a workflow artifact, so you can upload it by hand.

## Release flow

1. Push a stable tag, e.g. `v0.3.0`. The **Release** workflow builds every platform into a draft release.
2. Review and **Publish** the draft on GitHub.
3. Publishing triggers **Microsoft Store**: it builds the MSIX bundle and creates a Store submission.
4. Microsoft certifies the submission (usually 1–3 days); once it passes, Store users are updated automatically.

## Notes

- Store version numbers are `major.minor.patch.0`. Every Store submission must have a higher version than the last, so bump the version for each release.
- The MSIX runs as a full-trust desktop app, so trash, file watching, search indexing and the global shortcut work the same as the regular installer.
- WebView2 ships with Windows 11 and current Windows 10, which the Store package relies on.
