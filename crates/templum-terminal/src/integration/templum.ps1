# Templum's shell integration for PowerShell (7 and Windows PowerShell 5.1), started with
# `-NoExit -Command` reading this file. It tells Templum when a prompt starts (OSC 133 A), when a
# command starts and ends (C and D, with the exit code) and the current folder (OSC 7):
# PowerShell's `cd` doesn't move its process, so Templum can't ask the system for it. It wraps
# the prompt and, with PSReadLine, reading the command line. Parts follow VS Code's
# shellIntegration.ps1 (MIT, (c) Microsoft).

if ($null -eq $Global:__TemplumPrompt) {
    $Global:__TemplumPrompt = $function:prompt
    $Global:__TemplumRunning = $false

    function Global:prompt {
        $ok = $global:?
        Set-StrictMode -Off
        $code = $global:LASTEXITCODE
        $esc = [char]27
        $bel = [char]7
        $out = ''
        if ($Global:__TemplumRunning) {
            $Global:__TemplumRunning = $false
            $exit = if ($ok) { 0 } elseif ($code) { $code } else { 1 }
            $out += "$esc]133;D;$exit$bel"
        }
        if ($PWD.Provider.Name -eq 'FileSystem') {
            $dir = $PWD.ProviderPath.Replace('\', '/').Replace('%', '%25')
            if (-not $dir.StartsWith('/')) { $dir = "/$dir" }
            $out += "$esc]7;file://$([Environment]::MachineName)$dir$bel"
        }
        $out += "$esc]133;A$bel"
        # The prompt it wraps sees the command's status, as VS Code does it: $? is read-only.
        $global:LASTEXITCODE = $code
        if (-not $ok) { Write-Error 'failure' -ErrorAction Ignore }
        $out + $Global:__TemplumPrompt.Invoke()
    }

    if (Get-Module PSReadLine) {
        $Global:__TemplumReadLine = $function:PSConsoleHostReadLine
        function Global:PSConsoleHostReadLine {
            $line = [string]$Global:__TemplumReadLine.Invoke()
            if ($line.Trim()) {
                $Global:__TemplumRunning = $true
                [Console]::Write("$([char]27)]133;C$([char]7)")
            }
            $line
        }
    }
}
