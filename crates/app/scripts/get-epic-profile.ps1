<#
.SYNOPSIS
    Saves your Fortnite quest profile to epic-profile.json for Fortnite Chest Counter.

.DESCRIPTION
    Exchanges a one-time code from epicgames.com for a short-lived session, downloads
    your Battle Royale profile (quests and their completion times) and closes the
    session immediately. Only the profile file is written; no code or token is kept.
#>

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# The Fortnite PC game client. These values ship inside every Fortnite install.
$ClientId = 'ec684b8c687f479fadea3cb2ad83f5c6'
$ClientSecret = 'e1f31c211f28413186262d37a13fc84d'

$AccountService = 'https://account-public-service-prod.ol.epicgames.com/account/api/oauth'
$ProfileService = 'https://mcp-gc.live.fngw.ol.epicgames.com/fortnite/api/game/v2/profile'
$CodePage = "https://www.epicgames.com/id/api/redirect?clientId=$ClientId&responseType=code"
$OutFile = Join-Path $PSScriptRoot 'epic-profile.json'

function Get-ErrorText($err) {
    if ($err.ErrorDetails -and $err.ErrorDetails.Message) {
        try { return ($err.ErrorDetails.Message | ConvertFrom-Json).errorMessage } catch { return $err.ErrorDetails.Message }
    }
    return $err.Exception.Message
}

Write-Host ''
Write-Host 'Fortnite Chest Counter - download your quest profile' -ForegroundColor Yellow
Write-Host ''
Write-Host '1. Your browser opens an Epic Games page. Sign in if it asks you to.'
Write-Host '2. The page shows a line of text containing "authorizationCode".'
Write-Host '   Copy the whole page (Ctrl+A, Ctrl+C) or just the 32-character code.'
Write-Host ''
Start-Process $CodePage

$pasted = Read-Host 'Paste it here and press Enter'
# The page has other 32-character ids too, so take the code by its name, then from the
# redirect link, then a bare code pasted on its own.
$patterns = '"authorizationCode"\s*:\s*"([0-9a-fA-F]{32})"', '[?&]code=([0-9a-fA-F]{32})', '^\s*([0-9a-fA-F]{32})\s*$'
$code = ''
foreach ($pattern in $patterns) {
    $code = [regex]::Match($pasted, $pattern).Groups[1].Value
    if ($code) { break }
}
if (-not $code) {
    Write-Host 'No code found in what you pasted. Run the script again.' -ForegroundColor Red
    Read-Host 'Press Enter to close'
    exit 1
}

$basic = [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes("${ClientId}:${ClientSecret}"))
$session = $null
try {
    $session = Invoke-RestMethod -Method Post -Uri "$AccountService/token" `
        -Headers @{ Authorization = "basic $basic" } `
        -ContentType 'application/x-www-form-urlencoded' `
        -Body @{ grant_type = 'authorization_code'; code = $code }

    $response = Invoke-WebRequest -UseBasicParsing -Method Post `
        -Uri "$ProfileService/$($session.account_id)/client/QueryProfile?profileId=athena&rvn=-1" `
        -Headers @{ Authorization = "bearer $($session.access_token)" } `
        -ContentType 'application/json' -Body '{}'

    [IO.File]::WriteAllBytes($OutFile, $response.RawContentStream.ToArray())
    Write-Host ''
    Write-Host "Saved: $OutFile" -ForegroundColor Green
    Write-Host 'In Fortnite Chest Counter, click "Import epic-profile.json".'
}
catch {
    Write-Host ''
    Write-Host "Failed: $(Get-ErrorText $_)" -ForegroundColor Red
    Write-Host 'Codes expire after a few minutes and work once. Run the script again for a new one.'
}
finally {
    if ($session) {
        try {
            Invoke-RestMethod -Method Delete -Uri "$AccountService/sessions/kill/$($session.access_token)" `
                -Headers @{ Authorization = "bearer $($session.access_token)" } | Out-Null
            Write-Host 'The Epic session was closed.'
        }
        catch {
            Write-Host 'Could not close the Epic session; it expires on its own within a few hours.'
        }
    }
}

Write-Host ''
Read-Host 'Press Enter to close'
