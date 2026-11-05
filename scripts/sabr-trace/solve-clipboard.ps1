# Local half of the `n` validation. Waits until the clipboard holds a JSON array of `n` challenge
# strings (copy(JSON.stringify(__nv.received())) in the page), solves them with the QuickJS oracle
# against the cached player, and puts [[in,out],...] back on the clipboard for __nv.compare.
# Values only travel clipboard -> this process -> clipboard; nothing is printed, logged or saved.
# Every message below is a count or a fixed sentence, never a value.
param(
    [string]$Solver = (Join-Path $env:LOCALAPPDATA 'VibeDeck\ejs-solver'),
    # Puts validate-n.js on the clipboard first, so a single window drives the whole flow.
    [switch]$Install
)
$ErrorActionPreference = 'Stop'
$OutputEncoding = [System.Text.UTF8Encoding]::new($false) # no BOM on the pipe to the native tool
Set-Location (Join-Path $PSScriptRoot '..\..')
$exe = Join-Path (Get-Location) 'tools\decipher-eval\target\debug\examples\ejs_oracle.exe'
if (-not (Test-Path $exe)) { throw "ejs_oracle.exe not built: $exe" }
if (-not (Test-Path (Join-Path $Solver 'core.min.js'))) { throw "solver not found in $Solver" }

if ($Install) {
    Set-Clipboard -Value (Get-Content -Raw -Encoding UTF8 (Join-Path $PSScriptRoot 'validate-n.js'))
    Write-Host 'Hook copied. Paste it (Ctrl+V) in the Console of music.youtube.com and press Enter.'
    Write-Host 'Then play a NEW track for 30-60 s, and TYPE in the Console: copy(JSON.stringify(__nv.received()))'
}

$item = '"[A-Za-z0-9_-]{8,64}"'
$pattern = "^\[$item(,$item)*\]$"
$looksLikeList = '^\[\s*"[^\[\]]*"\s*(,\s*"[^\[\]]*"\s*)*\]$'
$warned = $false
Write-Host 'Waiting for the received n list on the clipboard...'
do {
    Start-Sleep -Milliseconds 500
    $text = Get-Clipboard -Raw
    if ($text -and -not $warned -and $text.Trim() -match $looksLikeList -and $text.Trim() -notmatch $pattern) {
        $bad = ([regex]::Matches($text, '"[^"]*"') | Where-Object { $_.Value -notmatch "^$item$" }).Count
        Write-Host "The clipboard has a list, but $bad entr(y/ies) are not 8-64 chars of A-Z a-z 0-9 _ - . Still waiting."
        $warned = $true
    }
} until ($text -and $text.Trim() -match $pattern)

$count = ([regex]::Matches($text, $item)).Count
Write-Host "Got $count challenge(s). Solving (about a minute)..."
$out = ($text.Trim() | & $exe $Solver --stdin) -join ''
if ($LASTEXITCODE -ne 0) {
    Write-Host "Solver failed (exit $LASTEXITCODE): $out"
    exit 1
}
if ($out -eq '[]') {
    Write-Host 'The solver returned no usable pair for these challenges. The clipboard still has the list, so do NOT paste it into compare.'
    exit 1
}
if ($out -notmatch '^\[\[') {
    Write-Host "Unexpected solver output (it starts with '$($out.Substring(0, [Math]::Min(1, $out.Length)))'). The clipboard was NOT changed."
    exit 1
}
Set-Clipboard -Value $out
$pairs = ([regex]::Matches($out, '\["')).Count
Write-Host "Done: $pairs pair(s) for $count challenge(s) are on the clipboard. Paste them into compare now."
