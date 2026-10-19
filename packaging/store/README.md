# Microsoft Store listing

Everything the Store shows for Isothrum lives here and goes out with each release
(`.github/workflows/msix.yml` → `.github/scripts/store-submit.ps1`, Partner Center submission API):

- `listings/<lang>.json`: title, descriptions, "What's new" (`releaseNotes`), features, search terms,
  links, screenshot captions. One file per language (`en-us`); add a language by adding a file (and
  a matching `Strings/<lang>/Resources.resw` + `<Resource Language="…">` in the MSIX manifest).
- `listings/common.json`: the `notesForCertification` the certification team reads, plus the names
  of the shared box art and tile.
- `images/<lang>/`: screenshots (`screenshot-N.png`, 16:9, ≥1366×768) and the 9:16 poster.
- `images/common/`: 1:1 box art (2160×2160) and the 300×300 app tile, shared by every language.
- `trailers/<lang>/`: `trailer.mp4` (1920×1080, H.264, <60 s) and its `thumbnail.png` (1920×1080).

**The listing text is sent on every release; images and trailers are optional.** Until you add the
files under `images/` and `trailers/`, the submission updates the package and the text and leaves
the screenshots/logos/trailer as they are in Partner Center (set them by hand there the first time).
Once the files exist here they flow automatically. Trailers need an audio track (the Store refuses a
silent video without one): add a silent stereo AAC track if the video has none.

The package itself (x64 + Arm64 `.msixbundle`) declares the app's languages and description from
`packaging/msix/` (manifest + `Strings/<lang>/Resources.resw`).

## Owner setup (one time)

1. Create the Isothrum listing in Partner Center and complete its **first** submission MANUALLY
   (the API cannot complete the first submission on an incomplete listing — CI automates the rest).
2. Copy the exact **Identity Name** and **Publisher** (`CN=…`) from Partner Center >
   Product Identity into `packaging/msix/AppxManifest.xml`.
3. Set the repository **variable** `STORE_PRODUCT_ID` to Isothrum's Store ID (Product Identity).
   Without it, the Store steps no-op (the GitHub/installer release is unaffected).
4. Set the repository **secrets** `STORE_TENANT_ID`, `STORE_CLIENT_ID`, `STORE_CLIENT_SECRET`
   (the Entra app associated in Partner Center with the Manager role).

Confirm `websiteUrl`, `privacyPolicy` and `supportContact` in each listing resolve before tagging.
Update `releaseNotes` in each listing before tagging a release. Set the repository variable
`STORE_AUTO_SUBMIT=false` to have a release leave its Store submission as a draft to review in
Partner Center instead of sending it for certification.
