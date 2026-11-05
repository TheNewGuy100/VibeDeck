[CmdletBinding()]
param(
    [switch]$Help
)

$ErrorActionPreference = 'Stop'

if ($Help) {
    Write-Output 'Usage: npm run youtube:healthcheck (requires an already running VibeDeck debug app; optional VIBEDECK_CDP_PORT, default 9222).'
    Write-Output 'Runs one authenticated YouTube Music search, reads playlist metadata, and prints safe API checks plus player/audio/UMP-capture diagnostics.'
    Write-Output 'The search may appear in account search history according to YouTube settings.'
    exit 0
}

[Console]::Error.WriteLine('This runs one authenticated YouTube Music search and reads playlist metadata. The search may appear in account search history according to YouTube settings.')

$timeoutMs = 120000
$client = $null
$stage = 'startup'

function Stop-Safely([string]$Message) {
    [Console]::Error.WriteLine($Message)
    exit 1
}

function ConvertTo-SafePlayerSummary($Player) {
    if ($null -eq $Player) { return $null }

    $allowedStatuses = @('idle', 'playing', 'paused', 'stopped')
    $allowedSources = @('youtube', 'local', 'other', 'none')
    $status = [string]$Player.status
    $source = [string]$Player.source
    if ($status -notin $allowedStatuses -or $source -notin $allowedSources -or $Player.nativeMode -isnot [bool]) { return $null }

    $numericFields = @('positionSeconds', 'durationSeconds', 'queueLength', 'queueIndex', 'queueGeneration', 'activationSequence')
    $safe = [ordered]@{ status = $status; source = $source }
    foreach ($field in $numericFields) {
        $raw = $Player.$field
        if ($null -eq $raw -or $raw -isnot [ValueType] -or $raw -is [bool]) { return $null }
        $number = 0.0
        $minimum = if ($field -eq 'queueIndex') { -1.0 } else { 0.0 }
        if (-not [double]::TryParse([string]$raw, [Globalization.NumberStyles]::Float, [Globalization.CultureInfo]::InvariantCulture, [ref]$number) -or [double]::IsNaN($number) -or [double]::IsInfinity($number) -or $number -lt $minimum) { return $null }
        if ($field -notin @('positionSeconds', 'durationSeconds') -and [Math]::Floor($number) -ne $number) { return $null }
        $safe[$field] = $number
    }
    $safe.nativeMode = $Player.nativeMode
    return [pscustomobject]$safe
}

function Test-SafeInteger($Value, [long]$Maximum = [long]::MaxValue) {
    if ($null -eq $Value -or $Value -is [bool] -or $Value -isnot [ValueType]) { return $false }
    $number = 0.0
    return [double]::TryParse([string]$Value, [Globalization.NumberStyles]::Float, [Globalization.CultureInfo]::InvariantCulture, [ref]$number) -and
        -not [double]::IsNaN($number) -and -not [double]::IsInfinity($number) -and
        $number -ge 0 -and $number -le $Maximum -and [Math]::Floor($number) -eq $number
}

function ConvertTo-SafePartKinds($Parts) {
    if ($null -eq $Parts) { return ,@() }
    $items = @($Parts)
    if ($items.Count -gt 32) { throw 'invalid parts' }
    $safe = @()
    foreach ($part in $items) {
        if ($null -eq $part -or -not (Test-SafeInteger $part.kind ([long]::MaxValue)) -or -not (Test-SafeInteger $part.count ([long]::MaxValue))) { throw 'invalid parts' }
        $safe += [pscustomobject][ordered]@{ kind = [long]$part.kind; count = [long]$part.count }
    }
    return ,$safe
}

function ConvertTo-SafeRequestContext($Context) {
    if ($null -eq $Context) { return $null }
    $methods = @('GET', 'POST', 'other')
    $mimeTypes = @('audio/mp4', 'audio/webm', 'other', 'unknown')
    if ([string]$Context.method -notin $methods -or [string]$Context.mimeType -notin $mimeTypes -or
        $Context.useUmp -isnot [bool] -or $Context.potPresent -isnot [bool] -or $Context.cpnPresent -isnot [bool]) { return $null }
    $safe = [ordered]@{
        method = [string]$Context.method
        useUmp = $Context.useUmp
        mimeType = [string]$Context.mimeType
        potPresent = $Context.potPresent
        cpnPresent = $Context.cpnPresent
    }
    foreach ($field in @('itag', 'rangeStart', 'rangeEnd', 'contentLength', 'rn', 'rbuf')) {
        if ($null -ne $Context.$field) {
            if (-not (Test-SafeInteger $Context.$field ([long]::MaxValue))) { return $null }
            $safe[$field] = [long]$Context.$field
        }
    }
    if (($null -eq $Context.rangeStart) -xor ($null -eq $Context.rangeEnd)) { return $null }
    if ($null -ne $Context.rangeStart -and [long]$Context.rangeStart -gt [long]$Context.rangeEnd) { return $null }
    return [pscustomobject]$safe
}

function ConvertTo-SafeRequestHeaders($Headers) {
    if ($null -eq $Headers) { return $null }
    $headerNames = @($Headers.headerNames)
    if ($headerNames.Count -gt 64) { return $null }
    foreach ($name in $headerNames) {
        if ([string]$name -cnotmatch '^[a-z0-9-]{1,64}$') { return $null }
    }
    $origins = @('music_youtube', 'other', 'missing')
    $families = @('chrome', 'edge', 'firefox', 'safari', 'other', 'missing')
    $contentTypes = @('form_urlencoded', 'text_plain', 'other', 'missing')
    $acceptValues = @('wildcard', 'audio', 'json', 'other', 'missing')
    $encodingValues = @('gzip_br', 'gzip', 'br', 'zstd', 'other', 'missing')
    $fetchSites = @('same_origin', 'same_site', 'cross_site', 'none', 'other', 'missing')
    if ([string]$Headers.origin -notin $origins -or [string]$Headers.referer -notin $origins -or
        [string]$Headers.userAgentFamily -notin $families -or [string]$Headers.contentType -notin $contentTypes -or
        [string]$Headers.accept -notin $acceptValues -or [string]$Headers.acceptEncoding -notin $encodingValues -or
        [string]$Headers.secFetchSite -notin $fetchSites) { return $null }
    foreach ($field in @('cookiePresent', 'authorizationPresent', 'secChUaPresent', 'secChUaMobilePresent', 'secChUaPlatformPresent', 'xGoogVisitorIdPresent', 'xGoogAuthuserPresent')) {
        if ($Headers.$field -isnot [bool]) { return $null }
    }
    $safe = [ordered]@{
        headerNames = @($headerNames | ForEach-Object { [string]$_ })
        origin = [string]$Headers.origin
        referer = [string]$Headers.referer
        userAgentFamily = [string]$Headers.userAgentFamily
        contentType = [string]$Headers.contentType
        accept = [string]$Headers.accept
        acceptEncoding = [string]$Headers.acceptEncoding
        secFetchSite = [string]$Headers.secFetchSite
    }
    if ($null -ne $Headers.userAgentMajor) {
        if (-not (Test-SafeInteger $Headers.userAgentMajor 999)) { return $null }
        $safe.userAgentMajor = [int]$Headers.userAgentMajor
    }
    if ($null -ne $Headers.contentLength) {
        if (-not (Test-SafeInteger $Headers.contentLength ([long]::MaxValue))) { return $null }
        $safe.contentLength = [long]$Headers.contentLength
    }
    foreach ($field in @('cookiePresent', 'authorizationPresent', 'secChUaPresent', 'secChUaMobilePresent', 'secChUaPlatformPresent', 'xGoogVisitorIdPresent', 'xGoogAuthuserPresent')) {
        $safe[$field] = $Headers.$field
    }
    return [pscustomobject]$safe
}

function ConvertTo-SafeAudioSummary($Audio) {
    if ($null -eq $Audio) { return $null }
    $stages = @('idle', 'activation', 'request_start', 'response_received', 'ump_parsing', 'media_download', 'container_probe', 'codec_init', 'output_create', 'output_start', 'decoding', 'failed', 'ended', 'cancelled')
    $outcomes = @('none', 'in_progress', 'error', 'ended', 'cancelled')
    $sources = @('youtube', 'local', 'none')
    $errors = @('http_400_ump', 'http_403', 'http_error', 'invalid_response', 'http3_not_negotiated', 'ump_decode_error', 'output_device_error', 'output_start_error', 'network_error', 'container_probe_error', 'codec_error', 'cancelled')
    $contentTypes = @('audio/mp4', 'audio/webm', 'application/vnd.yt-ump', 'application/octet-stream', 'application/json', 'text/plain', 'unknown', 'other')
    if ([string]$Audio.stage -notin $stages -or [string]$Audio.outcome -notin $outcomes -or [string]$Audio.source -notin $sources -or
        $Audio.decoderStarted -isnot [bool] -or $Audio.outputStarted -isnot [bool] -or
        -not $Audio.PSObject.Properties['umpPartKinds'] -or $null -eq $Audio.umpPartKinds) { return $null }
    $safe = [ordered]@{ activationSequence = 0; stage = [string]$Audio.stage; outcome = [string]$Audio.outcome; source = [string]$Audio.source }
    foreach ($field in @('activationSequence', 'responseBytes', 'mediaBytes', 'updatedAtUnixMs')) {
        if (-not (Test-SafeInteger $Audio.$field ([long]::MaxValue))) { return $null }
        $safe[$field] = [long]$Audio.$field
    }
    if ($null -ne $Audio.playerActivationSequence) {
        if (-not (Test-SafeInteger $Audio.playerActivationSequence ([long]::MaxValue))) { return $null }
        $safe.playerActivationSequence = [long]$Audio.playerActivationSequence
    }
    if ($null -ne $Audio.requestBody) {
        if ($null -ne $Audio.requestBody.byteLength -and -not (Test-SafeInteger $Audio.requestBody.byteLength 64)) { return $null }
        if ([string]$Audio.requestBody.category -notin @('x', 'x_equals', 'x_nul', 'x_backslash', 'x_space', 'x_cr', 'x_lf', 'empty', 'other', 'unavailable', 'printable_ascii', 'ascii_control_or_text', 'non_ascii')) { return $null }
        $safe.requestBody = [ordered]@{ category = [string]$Audio.requestBody.category }
        if ($null -ne $Audio.requestBody.byteLength) { $safe.requestBody.byteLength = [long]$Audio.requestBody.byteLength }
    }
    if ($null -ne $Audio.httpStatus) {
        if (-not (Test-SafeInteger $Audio.httpStatus 599) -or [long]$Audio.httpStatus -lt 100) { return $null }
        $safe.httpStatus = [int]$Audio.httpStatus
    }
    if ($null -ne $Audio.responseVersion) {
        if ([string]$Audio.responseVersion -notin @('http_09', 'http_10', 'http_11', 'http_2', 'http_3', 'unknown')) { return $null }
        $safe.responseVersion = [string]$Audio.responseVersion
    }
    if ($null -ne $Audio.contentType) {
        $contentType = [string]$Audio.contentType
        if ($contentType -notin $contentTypes) { return $null }
        $safe.contentType = $contentType
    }
    try { $safe.umpPartKinds = ConvertTo-SafePartKinds $Audio.umpPartKinds } catch { return $null }
    if ($null -ne $Audio.requestContext) {
        $requestContext = ConvertTo-SafeRequestContext $Audio.requestContext
        if (-not $requestContext) { return $null }
        $safe.requestContext = $requestContext
    }
    if ($null -ne $Audio.requestHeaders) {
        $requestHeaders = ConvertTo-SafeRequestHeaders $Audio.requestHeaders
        if (-not $requestHeaders) { return $null }
        $safe.requestHeaders = $requestHeaders
    }
    if ($null -ne $Audio.sameStreamRequest) {
        if ($Audio.sameStreamRequest -isnot [bool]) { return $null }
        $safe.sameStreamRequest = $Audio.sameStreamRequest
    }
    if ($null -ne $Audio.errorCode) {
        $errorCode = [string]$Audio.errorCode
        if ($errorCode -notin $errors) { return $null }
        $safe.errorCode = $errorCode
    }
    $safe.decoderStarted = $Audio.decoderStarted
    $safe.outputStarted = $Audio.outputStarted
    return [pscustomobject]$safe
}

function ConvertTo-SafeWebviewCaptureSummary($Capture) {
    if ($null -eq $Capture) { return $null }
    $stages = @('waiting', 'capture', 'captured')
    $outcomes = @('not_attempted', 'in_progress', 'success', 'capture_failed', 'capture_setup_failed', 'get_content_failed', 'content_stream_unavailable', 'response_too_large', 'content_stream_read_failed', 'get_content_start_failed', 'capture_channel_disconnected', 'timeout', 'unavailable')
    $contentTypes = @('application/vnd.yt-ump', 'other')
    if ([string]$Capture.stage -notin $stages -or [string]$Capture.outcome -notin $outcomes -or
        -not $Capture.PSObject.Properties['partKinds'] -or $null -eq $Capture.partKinds) { return $null }
    $safe = [ordered]@{ stage = [string]$Capture.stage; outcome = [string]$Capture.outcome }
    if ($Capture.scope -ne 'webview_capture_only_not_audio_engine_input_proof') { return $null }
    $safe.scope = 'webview_capture_only_not_audio_engine_input_proof'
    if ($null -ne $Capture.activationSequence) {
        if (-not (Test-SafeInteger $Capture.activationSequence ([long]::MaxValue))) { return $null }
        $safe.activationSequence = [long]$Capture.activationSequence
    }
    foreach ($field in @('bodyBytes', 'mediaBytes')) {
        if (-not (Test-SafeInteger $Capture.$field ([long]::MaxValue))) { return $null }
        $safe[$field] = [long]$Capture.$field
    }
    if ($null -ne $Capture.httpStatus) {
        if (-not (Test-SafeInteger $Capture.httpStatus 599) -or [long]$Capture.httpStatus -lt 100) { return $null }
        $safe.httpStatus = [int]$Capture.httpStatus
    }
    if ($null -ne $Capture.contentType) {
        $contentType = [string]$Capture.contentType
        if ($contentType -notin $contentTypes) { return $null }
        $safe.contentType = $contentType
    }
    try { $safe.partKinds = ConvertTo-SafePartKinds $Capture.partKinds } catch { return $null }
    if ($null -ne $Capture.rustParserOutcome) {
        if ([string]$Capture.rustParserOutcome -notin @('media_extracted', 'no_complete_media', 'parse_error')) { return $null }
        $safe.rustParserOutcome = [string]$Capture.rustParserOutcome
    }
    if ($null -ne $Capture.rustExtractedMediaBytes) {
        if (-not (Test-SafeInteger $Capture.rustExtractedMediaBytes ([long]::MaxValue))) { return $null }
        $safe.rustExtractedMediaBytes = [long]$Capture.rustExtractedMediaBytes
    }
    if ($null -ne $Capture.requestContext) {
        $requestContext = ConvertTo-SafeRequestContext $Capture.requestContext
        if (-not $requestContext) { return $null }
        $safe.requestContext = $requestContext
    }
    if ($null -ne $Capture.requestHeaders) {
        $requestHeaders = ConvertTo-SafeRequestHeaders $Capture.requestHeaders
        if (-not $requestHeaders) { return $null }
        $safe.requestHeaders = $requestHeaders
    }
    if ($null -ne $Capture.requestBody) {
        if ([string]$Capture.requestBody.category -notin @('x', 'x_equals', 'x_nul', 'x_backslash', 'x_space', 'x_cr', 'x_lf', 'empty', 'other', 'unavailable', 'printable_ascii', 'ascii_control_or_text', 'non_ascii')) { return $null }
        $safe.requestBody = [ordered]@{ category = [string]$Capture.requestBody.category }
        if ($null -ne $Capture.requestBody.byteLength) {
            if (-not (Test-SafeInteger $Capture.requestBody.byteLength 64)) { return $null }
            $safe.requestBody.byteLength = [long]$Capture.requestBody.byteLength
        }
    }
    if ($null -ne $Capture.sameStreamRequest) {
        if ($Capture.sameStreamRequest -isnot [bool]) { return $null }
        $safe.sameStreamRequest = $Capture.sameStreamRequest
    }
    if ($null -ne $Capture.usedCaptureAsPlaybackUrl) {
        if ($Capture.usedCaptureAsPlaybackUrl -isnot [bool]) { return $null }
        $safe.usedCaptureAsPlaybackUrl = $Capture.usedCaptureAsPlaybackUrl
    }
    return [pscustomobject]$safe
}

function Receive-CdpMessage([System.Net.WebSockets.ClientWebSocket]$Socket, [int]$Timeout) {
    $buffer = New-Object byte[] 8192
    $stream = New-Object System.IO.MemoryStream
    $cancel = New-Object System.Threading.CancellationTokenSource($Timeout)
    try {
        do {
            $segment = [ArraySegment[byte]]::new($buffer)
            $result = $Socket.ReceiveAsync($segment, $cancel.Token).GetAwaiter().GetResult()
            if ($result.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) {
                throw 'closed'
            }
            $stream.Write($buffer, 0, $result.Count)
        } while (-not $result.EndOfMessage)
        return [System.Text.Encoding]::UTF8.GetString($stream.ToArray())
    }
    finally {
        $cancel.Dispose()
        $stream.Dispose()
    }
}

try {
    $port = 9222
    if ($env:VIBEDECK_CDP_PORT) {
        if (-not [int]::TryParse($env:VIBEDECK_CDP_PORT, [ref]$port) -or $port -lt 1 -or $port -gt 65535) {
            Stop-Safely 'Invalid VIBEDECK_CDP_PORT; set it to the debug app CDP port (default 9222).'
        }
    }

    $stage = 'target discovery'
    $debugRoot = "http://127.0.0.1:$port"
    $version = Invoke-RestMethod -Uri "$debugRoot/json/version" -TimeoutSec ([Math]::Ceiling($timeoutMs / 1000))
    $stage = 'socket connect'
    $client = New-Object System.Net.WebSockets.ClientWebSocket
    $client.Options.KeepAliveInterval = [TimeSpan]::FromSeconds(20)
    $null = $client.ConnectAsync([Uri]$version.webSocketDebuggerUrl, [Threading.CancellationToken]::None).GetAwaiter().GetResult()

    $stage = 'target discovery'
    $targetRequest = @{ id = 1; method = 'Target.getTargets' } | ConvertTo-Json -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($targetRequest)
    $null = $client.SendAsync([ArraySegment[byte]]::new($bytes), [System.Net.WebSockets.WebSocketMessageType]::Text, $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
    $targetReply = $null
    $targetDeadline = [DateTime]::UtcNow.AddMilliseconds($timeoutMs)
    while ([DateTime]::UtcNow -lt $targetDeadline) {
        $left = [Math]::Max(1, [int]($targetDeadline - [DateTime]::UtcNow).TotalMilliseconds)
        $message = Receive-CdpMessage $client $left | ConvertFrom-Json
        if ($message.id -eq 1) { $targetReply = $message; break }
    }
    $target = @($targetReply.result.targetInfos) | Where-Object {
        $_.type -eq 'page' -and ($_.url -match '^http://localhost:1420(?:/|$)' -or $_.title -eq 'VibeDeck')
    } | Select-Object -First 1
    if (-not $target -or $targetReply.error) { Stop-Safely 'No VibeDeck page target found. Start the debug app with npm run tauri:debug and retry.' }

    $stage = 'target attach'
    $attachRequest = @{ id = 2; method = 'Target.attachToTarget'; params = @{ targetId = [string]$target.targetId; flatten = $true } } | ConvertTo-Json -Depth 6 -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($attachRequest)
    $null = $client.SendAsync([ArraySegment[byte]]::new($bytes), [System.Net.WebSockets.WebSocketMessageType]::Text, $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()
    $attachReply = $null
    $attachDeadline = [DateTime]::UtcNow.AddMilliseconds($timeoutMs)
    while ([DateTime]::UtcNow -lt $attachDeadline) {
        $left = [Math]::Max(1, [int]($attachDeadline - [DateTime]::UtcNow).TotalMilliseconds)
        $message = Receive-CdpMessage $client $left | ConvertFrom-Json
        if ($message.id -eq 2) { $attachReply = $message; break }
    }
    if (-not $attachReply -or $attachReply.error -or -not $attachReply.result.sessionId) {
        Stop-Safely 'Could not attach to the VibeDeck debug target. Confirm the debug app is responsive and retry.'
    }
    $sessionId = [string]$attachReply.result.sessionId

    $stage = 'CDP initialization'
    $enable = @{ id = 3; sessionId = $sessionId; method = 'Runtime.enable' } | ConvertTo-Json -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($enable)
    $null = $client.SendAsync([ArraySegment[byte]]::new($bytes), [System.Net.WebSockets.WebSocketMessageType]::Text, $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()

    $stage = 'Tauri invoke'
    $expression = "(async()=>{const t=window.__TAURI_INTERNALS__;if(!t||typeof t.invoke!=='function')return {__healthcheck:'bridge_missing'};try{return await t.invoke('youtube_music_healthcheck')}catch(_){return {__healthcheck:'invoke_failed'}}})()"
    $request = @{ id = 4; sessionId = $sessionId; method = 'Runtime.evaluate'; params = @{ expression = $expression; awaitPromise = $true; returnByValue = $true } } | ConvertTo-Json -Depth 8 -Compress
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($request)
    $null = $client.SendAsync([ArraySegment[byte]]::new($bytes), [System.Net.WebSockets.WebSocketMessageType]::Text, $true, [Threading.CancellationToken]::None).GetAwaiter().GetResult()

    $stage = 'response wait'
    $deadline = [DateTime]::UtcNow.AddMilliseconds($timeoutMs)
    $reply = $null
    while ([DateTime]::UtcNow -lt $deadline) {
        $left = [Math]::Max(1, [int]($deadline - [DateTime]::UtcNow).TotalMilliseconds)
        $message = Receive-CdpMessage $client $left | ConvertFrom-Json
        if ($message.id -eq 4) { $reply = $message; break }
    }
    if (-not $reply) { Stop-Safely 'Healthcheck timed out. Confirm the debug app is responsive and retry.' }
    if ($reply.error -or $reply.result.exceptionDetails) { Stop-Safely 'Healthcheck command unavailable or failed. Rebuild/restart the debug app with the latest native command.' }

    $value = $reply.result.result.value
    if ($value.__healthcheck -eq 'bridge_missing') { Stop-Safely 'Tauri bridge unavailable. Start the VibeDeck debug app with npm run tauri:debug and retry.' }
    if ($value.__healthcheck -eq 'invoke_failed') { Stop-Safely 'Healthcheck command unavailable or failed. Rebuild/restart the debug app with the latest native command.' }
    if (-not $value -or $null -eq $value.checks) { Stop-Safely 'Healthcheck returned an unsupported report. Update the debug app and retry.' }

    $stage = 'report'
    $checks = @($value.checks)
    $summary = foreach ($check in $checks) {
        $operation = 'unknown'
        $status = 'unknown'
        if ([string]$check.operation -match '^[A-Za-z0-9_.-]{1,48}$') { $operation = [string]$check.operation }
        if ([string]$check.status -match '^[A-Za-z0-9_.-]{1,24}$') { $status = [string]$check.status }
        $item = [ordered]@{ operation = $operation; status = $status }
        $count = 0
        if ($null -ne $check.count -and [int]::TryParse([string]$check.count, [ref]$count) -and $count -ge 0) { $item.count = $count }
        $checkError = [string]$check.errorCode
        if ($checkError -in @('timeout', 'session_unavailable', 'request_failed')) { $item.errorCode = $checkError }
        elseif ($checkError -match '^http_(\d{3})$' -and [int]$Matches[1] -ge 100 -and [int]$Matches[1] -le 599) { $item.errorCode = $checkError }
        [pscustomobject]$item
    }
    $output = [ordered]@{ checks = @($summary) }
    $safePlayer = ConvertTo-SafePlayerSummary $value.player
    if ($safePlayer) {
        $output.player = $safePlayer
    }
    elseif ($null -ne $value.player) {
        [Console]::Error.WriteLine('Player snapshot omitted because it was unavailable or malformed.')
    }
    $safeAudio = ConvertTo-SafeAudioSummary $value.audio
    if ($safeAudio) { $output.audio = $safeAudio }
    elseif ($null -ne $value.audio) { [Console]::Error.WriteLine('Audio diagnostics omitted because they were malformed.') }
    $safeCapture = ConvertTo-SafeWebviewCaptureSummary $value.umpCapture
    if ($safeCapture) { $output.webviewCapture = $safeCapture }
    elseif ($null -ne $value.umpCapture) { [Console]::Error.WriteLine('Webview capture diagnostics omitted because they were malformed.') }
    if ($safeAudio -and $safeCapture -and $null -ne $safeAudio.playerActivationSequence -and $null -ne $safeCapture.activationSequence) {
        $output.sameActivationSequence = ($safeAudio.playerActivationSequence -eq $safeCapture.activationSequence)
    }
    if ($output.sameActivationSequence -and $safeAudio.requestHeaders -and $safeCapture.requestHeaders) {
        $headerFields = @('origin', 'referer', 'userAgentFamily', 'userAgentMajor', 'contentType', 'contentLength', 'accept', 'acceptEncoding', 'cookiePresent', 'authorizationPresent', 'secChUaPresent', 'secChUaMobilePresent', 'secChUaPlatformPresent', 'secFetchSite', 'xGoogVisitorIdPresent', 'xGoogAuthuserPresent')
        $differences = @($headerFields | Where-Object { $safeAudio.requestHeaders.$_ -ne $safeCapture.requestHeaders.$_ })
        $output.requestHeaderComparison = [ordered]@{
            nativeObservation = 'reqwest_request_before_execute'
            webviewObservation = 'webview2_request_from_successful_response_event'
            comparedFieldCount = $headerFields.Count
            differentFields = $differences
            nativeHeaderNames = @($safeAudio.requestHeaders.headerNames)
            webviewHeaderNames = @($safeCapture.requestHeaders.headerNames)
            missingInNative = @($safeCapture.requestHeaders.headerNames | Where-Object { $_ -notin $safeAudio.requestHeaders.headerNames })
            missingInWebview = @($safeAudio.requestHeaders.headerNames | Where-Object { $_ -notin $safeCapture.requestHeaders.headerNames })
        }
        if ($null -ne $safeAudio.requestBody.byteLength -and $null -ne $safeCapture.requestHeaders.contentLength) {
            $output.requestHeaderComparison.nativeBodyBytes = $safeAudio.requestBody.byteLength
            $output.requestHeaderComparison.webviewContentLength = $safeCapture.requestHeaders.contentLength
            $output.requestHeaderComparison.bodyLengthMatchesWebviewContentLength = ($safeAudio.requestBody.byteLength -eq $safeCapture.requestHeaders.contentLength)
        }
        if ($safeAudio.requestBody -and $safeCapture.requestBody) {
            $output.requestHeaderComparison.bodyCategoryMatches = ($safeAudio.requestBody.category -eq $safeCapture.requestBody.category)
            $output.requestHeaderComparison.webviewBodyCategory = $safeCapture.requestBody.category
            $output.requestHeaderComparison.nativeBodyCategory = $safeAudio.requestBody.category
        }
    }
    [pscustomobject]$output | ConvertTo-Json -Depth 5 -Compress | Write-Output
    if ($checks | Where-Object { $_.status -eq 'failed' }) { exit 2 }
}
catch {
    Stop-Safely "Healthcheck failed during $stage. Confirm the VibeDeck debug app is running and retry."
}
finally {
    if ($client) {
        if ($client.State -eq [System.Net.WebSockets.WebSocketState]::Open) {
            try { $null = $client.CloseAsync([System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure, 'done', [Threading.CancellationToken]::None).Wait(1000) } catch { }
        }
        $client.Dispose()
    }
}
