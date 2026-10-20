# Sends a release to the Microsoft Store through the Partner Center submission API: the package
# (an x64 + Arm64 .msixbundle) and the listing text, plus any screenshots, logos and trailers that
# are present in packaging/store/. Partner Center then shows a new submission:
#   -Commit $true   submits it for certification (release pipeline);
#   -Commit $false  leaves it as a draft to review and submit by hand in Partner Center.
#
# The package and the per-language listing text (packaging/store/listings/<lang>.json) are always
# sent. Images and trailers are OPTIONAL: a language's screenshots/poster/box-art/tile/trailer are
# uploaded only when their files exist in the repo, so the first releases (before those assets are
# committed) update the package + text and leave the images managed by hand in Partner Center. Once
# the files are added under packaging/store/images|trailers/, they flow automatically.
#
# NOTE: this script can CREATE and fill the very first submission as a reviewable DRAFT (it sets the
# required listing properties — applicationCategory, pricing, visibility, device families — that a
# first submission has no prior submission to copy them from, so they would otherwise arrive as
# 'NotSet' and the API would reject the update). It still cannot COMPLETE the first submission: the
# IARC age-rating questionnaire has no field in the submission API and must be answered once in
# Partner Center, and the first release-to-Store must be reviewed and submitted there by a person.
# So on a first run leave -Commit $false, finish the age rating in Partner Center, then submit it.
#
# Env: STORE_TENANT_ID, STORE_CLIENT_ID, STORE_CLIENT_SECRET, STORE_PRODUCT_ID.
# API reference: https://learn.microsoft.com/windows/uwp/monetize/manage-app-submissions
param(
  [Parameter(Mandatory)] [string] $Package,     # path to the .msixbundle
  [Parameter(Mandatory)] [string] $StoreDir,    # the repo's packaging/store/ folder
  [bool] $Commit = $false
)
$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($env:STORE_PRODUCT_ID)) {
  Write-Host "::warning::STORE_PRODUCT_ID is not set; skipping the Store submission."
  exit 0
}
$api = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$env:STORE_PRODUCT_ID"

function Get-Headers {
  $token = (Invoke-RestMethod -Method Post -Uri "https://login.microsoftonline.com/$env:STORE_TENANT_ID/oauth2/token" -Body @{
      grant_type = "client_credentials"; client_id = $env:STORE_CLIENT_ID; client_secret = $env:STORE_CLIENT_SECRET
      resource = "https://manage.devcenter.microsoft.com"
    }).access_token
  @{ Authorization = "Bearer $token" }
}
function Invoke-Store([string] $Method, [string] $Uri, $Body = $null) {
  $params = @{ Method = $Method; Uri = $Uri; Headers = (Get-Headers); ContentType = "application/json; charset=utf-8" }
  if ($null -ne $Body) { $params.Body = [Text.Encoding]::UTF8.GetBytes(($Body | ConvertTo-Json -Depth 30)) }
  $raw = Invoke-WebRequest @params
  if ($raw.Content) { $raw.Content | ConvertFrom-Json -AsHashtable } else { $null }
}

# Everything to upload goes flat into one zip; the submission refers to files by name.
$stage = Join-Path $env:RUNNER_TEMP "store-upload"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stage | Out-Null
function Add-File([string] $Source, [string] $Name) {
  Copy-Item $Source (Join-Path $stage $Name)
  return $Name
}

Write-Host "Creating a submission (a copy of the published one)"
$sub = Invoke-Store Post "$api/submissions"
$id = $sub.id
Write-Host "Submission $id"

# Required submission properties the API cannot default on a FIRST submission.
# A new submission is a COPY of the last published one, so these fields normally carry over. The
# very first submission has nothing to copy, so each enum below arrives as "NotSet" and the update
# is rejected ("InvalidParameterValue: 'NotSet' is not a valid value…"). Give each a valid value,
# but ONLY when it is unset/NotSet, so later submissions keep whatever the published listing had
# (idempotent — this block is a no-op once a real submission exists to copy from).
function Test-Unset($v) {
  return ($null -eq $v) -or (($v -is [string]) -and ([string]::IsNullOrWhiteSpace($v) -or $v -eq "NotSet"))
}

# Category: Isothrum builds and runs network labs on the developer's machine → "Developer tools".
if (Test-Unset $sub.applicationCategory) { $sub.applicationCategory = "DeveloperTools" }

# Pricing: Free, no trial, every market (an empty marketSpecificPricings leaves the base Free price
# in all markets). isAdvancedPricingModel is read-only — drop it so it is never sent back.
if (-not $sub.pricing) { $sub.pricing = @{} }
if (Test-Unset $sub.pricing.priceId) { $sub.pricing.priceId = "Free" }
if (Test-Unset $sub.pricing.trialPeriod) { $sub.pricing.trialPeriod = "NoFreeTrial" }
if ($null -eq $sub.pricing.marketSpecificPricings) { $sub.pricing.marketSpecificPricings = @{} }
if ($null -eq $sub.pricing.sales) { $sub.pricing.sales = @() }
if ($sub.pricing.ContainsKey("isAdvancedPricingModel")) { $sub.pricing.Remove("isAdvancedPricingModel") | Out-Null }

# Listed publicly once it ships; released by hand after certification (never auto-published).
if (Test-Unset $sub.visibility) { $sub.visibility = "Public" }
if (Test-Unset $sub.targetPublishMode) { $sub.targetPublishMode = "Manual" }

# Organizational licensing: offer through Store-managed (online) licensing (the Partner Center default).
if (Test-Unset $sub.enterpriseLicensing) { $sub.enterpriseLicensing = "Online" }

# Device families: Isothrum is a full-trust Win32 desktop app. Target Desktop, and let Microsoft
# decide about any future device families.
if ($null -eq $sub.allowMicrosoftDecideAppAvailabilityToFutureDeviceFamilies) {
  $sub.allowMicrosoftDecideAppAvailabilityToFutureDeviceFamilies = $true
}
if (-not $sub.allowTargetFutureDeviceFamilies) {
  $sub.allowTargetFutureDeviceFamilies = @{ Desktop = $true; Mobile = $false; Holographic = $false; Xbox = $false; Team = $false }
}

# Package: the new bundle replaces every package of the copied submission. minimumDirectXVersion /
# minimumSystemRam are required on update (ignored for MSIX, but must be present); "None" is the
# documented no-op value.
foreach ($p in $sub.applicationPackages) { $p.fileStatus = "PendingDelete" }
$sub.applicationPackages += @{ fileName = (Add-File $Package "Isothrum.msixbundle"); fileStatus = "PendingUpload"; minimumDirectXVersion = "None"; minimumSystemRam = "None" }

# Image types this script owns; any other image (set by hand in Partner Center) is left as is.
$managed = @("Screenshot", "StoreLogo9x16", "StoreLogoSquare", "Icon")
$common = Get-Content (Join-Path $StoreDir "listings/common.json") -Raw | ConvertFrom-Json -AsHashtable
# What the certification team reads first (why the restricted capabilities are needed).
if ($common.notesForCertification) { $sub.notesForCertification = $common.notesForCertification }
$trailers = @()

foreach ($file in Get-ChildItem (Join-Path $StoreDir "listings") -Filter "*-*.json") {
  $lang = $file.BaseName                       # en-us, fr-fr
  $l = Get-Content $file.FullName -Raw | ConvertFrom-Json -AsHashtable
  # A language without its own images yet falls back to the English ones.
  $img = Join-Path $StoreDir "images/$lang"
  if (-not (Test-Path $img)) { $img = Join-Path $StoreDir "images/en-us" }
  Write-Host "Listing $lang"

  if (-not $sub.listings) { $sub.listings = @{} }
  if (-not $sub.listings.ContainsKey($lang)) { $sub.listings[$lang] = @{ baseListing = @{ images = @() }; platformOverrides = @{} } }
  $b = $sub.listings[$lang].baseListing
  foreach ($k in "title", "shortDescription", "description", "releaseNotes", "features", "keywords", "copyrightAndTrademarkInfo", "websiteUrl", "privacyPolicy", "supportContact") {
    if ($l.ContainsKey($k)) { $b[$k] = $l[$k] }
  }

  # Collect only the images that actually exist on disk. If none do, the listing's images are left
  # exactly as they are in Partner Center (so manually-set screenshots are never wiped).
  $newImages = @()
  foreach ($s in @($l.screenshots)) {
    if ($s -and (Test-Path (Join-Path $img $s.file))) {
      $newImages += @{ source = (Join-Path $img $s.file); name = "$lang-$($s.file)"; meta = @{ imageType = "Screenshot"; description = $s.caption } }
    }
  }
  if ($l.poster -and (Test-Path (Join-Path $img $l.poster))) {
    $newImages += @{ source = (Join-Path $img $l.poster); name = "$lang-poster.png"; meta = @{ imageType = "StoreLogo9x16" } }
  }
  if ($common.boxArt -and (Test-Path (Join-Path $StoreDir "images/common/$($common.boxArt)"))) {
    $newImages += @{ source = (Join-Path $StoreDir "images/common/$($common.boxArt)"); name = "$lang-box-art.png"; meta = @{ imageType = "StoreLogoSquare" } }
  }
  if ($common.tile300 -and (Test-Path (Join-Path $StoreDir "images/common/$($common.tile300)"))) {
    $newImages += @{ source = (Join-Path $StoreDir "images/common/$($common.tile300)"); name = "$lang-tile-300.png"; meta = @{ imageType = "Icon" } }
  }

  if ($newImages.Count -gt 0) {
    $images = @()
    # Delete the managed images of the copied submission; re-upload the ones present in the repo.
    foreach ($i in @($b.images)) {
      if ($null -eq $i) { continue }
      if ($managed -contains $i.imageType) { $i.fileStatus = "PendingDelete" }
      $images += $i
    }
    foreach ($n in $newImages) {
      $images += (@{ fileName = (Add-File $n.source $n.name); fileStatus = "PendingUpload" } + $n.meta)
    }
    $b.images = $images
  } else {
    Write-Host "::notice::No local images for $lang; leaving its Store images as they are (manage them in Partner Center)."
  }

  # One trailer per language, shown on that language's page (its title and thumbnail live there).
  $t = Join-Path $StoreDir "trailers/$lang"
  if (-not (Test-Path $t)) { $t = Join-Path $StoreDir "trailers/en-us" }
  if ($l.trailer -and (Test-Path (Join-Path $t $l.trailer.video))) {
    $trailers += @{
      videoFileName = (Add-File (Join-Path $t $l.trailer.video) "$lang-trailer.mp4")
      trailerAssets = @{ $lang = @{ title = $l.trailer.title; imageList = @(@{ fileName = (Add-File (Join-Path $t $l.trailer.thumbnail) "$lang-trailer-thumbnail.png"); description = $l.trailer.title }) } }
    }
  }
}
# Only replace the trailers when the repo has at least one; otherwise leave those set by hand.
if ($trailers.Count -gt 0) { $sub.trailers = $trailers }

Write-Host "Updating submission $id"
Invoke-Store Put "$api/submissions/$id" $sub | Out-Null

Write-Host "Uploading $((Get-ChildItem $stage).Count) files"
$zip = Join-Path $env:RUNNER_TEMP "store-upload.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
# The upload URL is an Azure blob SAS; '+' in it must be escaped.
Invoke-WebRequest -Method Put -Uri ($sub.fileUploadUrl -replace '\+', '%2B') -Headers @{ "x-ms-blob-type" = "BlockBlob" } -InFile $zip | Out-Null

if (-not $Commit) {
  Write-Host "::notice::Submission $id is ready as a draft: review it in Partner Center (Isothrum > the open submission) and submit it there."
  exit 0
}

Write-Host "Submitting $id for certification"
Invoke-Store Post "$api/submissions/$id/commit" | Out-Null
do {
  Start-Sleep -Seconds 20
  $status = Invoke-Store Get "$api/submissions/$id/status"
  Write-Host "Status: $($status.status)"
} while ($status.status -eq "CommitStarted")
if ($status.status -eq "CommitFailed") {
  Write-Host "::error::The Store refused submission $($id): $(($status.statusDetails | ConvertTo-Json -Depth 10))"
  exit 1
}
Write-Host "Submission $id is in certification ($($status.status))."
