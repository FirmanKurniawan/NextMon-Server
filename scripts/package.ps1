<#
  NextMON Server - release packager.
  Builds the exe, stages lite/full/source zips, writes SHA256SUMS.txt.
  Staging and cargo output go to $Stage (default C:\nm-stage) to spare the project drive.
  Usage: powershell -NoProfile -File scripts\package.ps1 -Version 0.1.0 [-OutDir X:\mikhmon\release] [-SkipBuild] [-SkipFull]
#>
param(
  [string]$Version = '0.1.0',
  [string]$OutDir = 'X:\mikhmon\release',
  [string]$Stage = 'C:\nm-stage',
  [string]$ServerRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path,
  [string]$MonoRoot = 'X:\mikhmon',
  [switch]$SkipBuild,
  [string]$PhpRuntimeDir = (Join-Path $ServerRoot 'dist-dev\runtimes\php\8.5.12'),
  [switch]$SkipFull
)
$ErrorActionPreference = 'Stop'
function Step($m) { Write-Host "==> $m" -ForegroundColor Cyan }

$php = Join-Path $MonoRoot 'nextmon-php'
$next = Join-Path $MonoRoot 'nextmon-next'
$standalone = Join-Path $next '.next\standalone'
if (-not (Test-Path (Join-Path $php 'public\index.php'))) { throw "nextmon-php not found at $php" }

# 1. exe
$env:CARGO_TARGET_DIR = Join-Path $Stage 'target'
if (-not $SkipBuild) {
  Step 'cargo build --release'
  Push-Location $ServerRoot
  try { cargo build --release -p nextmon-server; if ($LASTEXITCODE) { throw 'cargo build failed' } } finally { Pop-Location }
}
$exeSrc = Join-Path $env:CARGO_TARGET_DIR 'release\nextmon-server.exe'
if (-not (Test-Path $exeSrc)) { throw "exe not found: $exeSrc" }

# 2. stage the lite tree
$name = "NextMON-Server-v$Version-win-x64"
$root = Join-Path $Stage "lite\$name"
if (Test-Path $root) { Remove-Item $root -Recurse -Force }
New-Item -ItemType Directory -Force $root | Out-Null

Step 'copy exe, ui, docs'
Copy-Item $exeSrc (Join-Path $root 'NextMON Server.exe')
Set-Content (Join-Path $root 'nextmon-server.cmd') '@"%~dp0NextMON Server.exe" %*' -Encoding ascii
Copy-Item (Join-Path $ServerRoot 'ui') (Join-Path $root 'ui') -Recurse
Copy-Item (Join-Path $ServerRoot 'docs') (Join-Path $root 'docs') -Recurse
Copy-Item (Join-Path $MonoRoot 'LICENSE') $root -ErrorAction SilentlyContinue

Step 'copy app/php (without data/tests)'
$appPhp = Join-Path $root 'app\php'
New-Item -ItemType Directory -Force $appPhp | Out-Null
foreach ($d in 'bin', 'deploy', 'lang', 'public', 'src') { Copy-Item (Join-Path $php $d) (Join-Path $appPhp $d) -Recurse }
Copy-Item (Join-Path $php 'cron.php') $appPhp
# per-install files must never ship
Get-ChildItem $appPhp -Recurse -Include '*.sqlite*', 'config.local.*' -ErrorAction SilentlyContinue | Remove-Item -Force

if (Test-Path (Join-Path $standalone 'server.js')) {
  Step 'copy app/next (standalone build)'
  $appNext = Join-Path $root 'app\next'
  New-Item -ItemType Directory -Force $appNext | Out-Null
  & robocopy $standalone $appNext /E /NFL /NDL /NJH /NJS /NP /XD data | Out-Null
  if ($LASTEXITCODE -ge 8) { throw 'robocopy app/next failed' }
} else {
  Write-Warning 'nextmon-next standalone build missing - run "npm run build" in nextmon-next; app/next NOT included'
}
Set-Content (Join-Path $root 'README.txt') @"
NextMON Server v$Version
Double-click "NextMON Server.exe". The panel opens; pick PHP or Next.js edition (one at a time).
Lite package: PHP/Node runtimes are downloaded from the panel (Runtime page) on first use.
Windows SmartScreen may warn: the exe is not code-signed (More info > Run anyway).
Verify the zip with SHA256SUMS.txt.
"@ -Encoding utf8

# 3. zips
New-Item -ItemType Directory -Force $OutDir | Out-Null
function Zip($srcDir, $zipPath) {
  if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
  Step "zip -> $zipPath"
  # bsdtar writes forward-slash entry names (Compress-Archive in PS 5.1 writes backslashes)
  & (Join-Path $env:SystemRoot "System32/tar.exe") -a -c -f $zipPath -C (Split-Path $srcDir) (Split-Path $srcDir -Leaf)
  if ($LASTEXITCODE) { throw "tar failed for $zipPath" }
}
$liteZip = Join-Path $OutDir "$name.zip"
Zip $root $liteZip

if (-not $SkipFull) {
  Step 'add runtimes to full tree'
  $fullName = "$name-full"
  $full = Join-Path $Stage "full\$fullName"
  if (Test-Path $full) { Remove-Item $full -Recurse -Force }
  New-Item -ItemType Directory -Force (Split-Path $full) | Out-Null
  Copy-Item $root $full -Recurse
  # PHP: an install previously made by the app (php.ini tuned, relative extension_dir)
  if (-not (Test-Path (Join-Path $PhpRuntimeDir 'php.exe'))) { throw "PHP runtime not found: $PhpRuntimeDir (install one from the panel first, or pass -PhpRuntimeDir)" }
  $phpDst = Join-Path $full (Join-Path 'runtimes\php' (Split-Path $PhpRuntimeDir -Leaf))
  New-Item -ItemType Directory -Force (Split-Path $phpDst) | Out-Null
  Copy-Item $PhpRuntimeDir $phpDst -Recurse
  Copy-Item (Join-Path (Split-Path (Split-Path $PhpRuntimeDir)) 'cacert.pem') (Join-Path $full 'runtimes') -ErrorAction SilentlyContinue
  # Node: official zip, checksum verified against SHASUMS256.txt
  $idx = Invoke-RestMethod 'https://nodejs.org/dist/index.json'
  $nv = ($idx | Where-Object { $_.lts } | Select-Object -First 1).version
  $zipName = "node-$nv-win-x64.zip"
  $tmp = Join-Path $Stage 'dl'; New-Item -ItemType Directory -Force $tmp | Out-Null
  $nz = Join-Path $tmp $zipName
  if (-not (Test-Path $nz)) { Invoke-WebRequest -UseBasicParsing "https://nodejs.org/dist/$nv/$zipName" -OutFile $nz }
  $sums = [string](Invoke-RestMethod "https://nodejs.org/dist/$nv/SHASUMS256.txt")
  $want = ($sums -split "`n" | Where-Object { $_ -match [regex]::Escape($zipName) + '\s*$' } | Select-Object -First 1).Split(' ')[0]
  $got = (Get-FileHash $nz -Algorithm SHA256).Hash.ToLower()
  if (-not $want -or $want -ne $got) { throw "node checksum mismatch ($want vs $got)" }
  $ex = Join-Path $tmp 'node-x'; if (Test-Path $ex) { Remove-Item $ex -Recurse -Force }
  Expand-Archive $nz $ex
  $nodeDst = Join-Path $full (Join-Path 'runtimes\node' $nv.TrimStart('v'))
  New-Item -ItemType Directory -Force (Split-Path $nodeDst) | Out-Null
  Move-Item (Get-ChildItem $ex | Select-Object -First 1).FullName $nodeDst
  Zip $full (Join-Path $OutDir "$fullName.zip")
}

# 4. source zip (no data, build output, node_modules, .git)
Step 'source zip'
$srcStage = Join-Path $Stage "src\NextMON-v$Version-source"
if (Test-Path $srcStage) { Remove-Item $srcStage -Recurse -Force }
New-Item -ItemType Directory -Force $srcStage | Out-Null
$xd = @('node_modules', '.next', '.next-*', 'target', 'dist-dev', 'data', '.git', '.claude', 'dist', 'coverage', '.test-tmp')
$xf = @('*.sqlite*', '*.log', 'config.json', '.env*')
& robocopy $MonoRoot $srcStage /E /NFL /NDL /NJH /NJS /NP /XD @($xd + @($OutDir)) /XF @xf | Out-Null
if ($LASTEXITCODE -ge 8) { throw 'robocopy source failed' }
$srvDst = Join-Path $srcStage 'nextmon-server'
& robocopy $ServerRoot $srvDst /E /NFL /NDL /NJH /NJS /NP /XD @($xd) /XF @xf | Out-Null
if ($LASTEXITCODE -ge 8) { throw 'robocopy server source failed' }
Zip $srcStage (Join-Path $OutDir "NextMON-v$Version-source.zip")

# 5. checksums
Step 'SHA256SUMS.txt'
$sums = Get-ChildItem $OutDir -Filter '*.zip' | ForEach-Object {
  "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
}
Set-Content (Join-Path $OutDir 'SHA256SUMS.txt') $sums -Encoding ascii
$sums
Write-Host "Done. Output: $OutDir" -ForegroundColor Green
