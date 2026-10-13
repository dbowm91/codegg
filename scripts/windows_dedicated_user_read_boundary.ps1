$ErrorActionPreference = 'Stop'

$runId = [Guid]::NewGuid().ToString('N')
$account = "c024_$($runId.Substring(0, 8))"
$accountCreated = $false
$root = Join-Path $env:RUNNER_TEMP "codegg-c024-$runId"
$stage = Join-Path $root 'stage'
$broadRoot = Join-Path $root 'broad-readable-host-fixture'
$privateRoot = Join-Path $root 'private-host-fixture'
$job = [IntPtr]::Zero
$baseToken = [IntPtr]::Zero
$writeRestrictedToken = [IntPtr]::Zero
$restrictedToken = [IntPtr]::Zero
$normalProcess = $null
$restrictedProcess = $null
$passwordSecure = $null

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct C024StartupInfo {
    public int cb;
    public string lpReserved, lpDesktop, lpTitle;
    public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars;
    public int dwFillAttribute, dwFlags;
    public short wShowWindow, cbReserved2;
    public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
}

[StructLayout(LayoutKind.Sequential)]
public struct C024ProcessInformation {
    public IntPtr hProcess, hThread;
    public uint dwProcessId, dwThreadId;
}

[StructLayout(LayoutKind.Sequential)]
public struct C024JobBasicLimitInformation {
    public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
    public uint LimitFlags;
    public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
    public uint ActiveProcessLimit;
    public UIntPtr Affinity;
    public uint PriorityClass, SchedulingClass;
}

[StructLayout(LayoutKind.Sequential)]
public struct C024IoCounters {
    public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
    public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
}

[StructLayout(LayoutKind.Sequential)]
public struct C024JobExtendedLimitInformation {
    public C024JobBasicLimitInformation BasicLimitInformation;
    public C024IoCounters IoInfo;
    public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
}

public static class C024Native {
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool CreateProcessWithLogonW(string user, string domain, string password,
        int logonFlags, string application, StringBuilder commandLine, int creationFlags,
        IntPtr environment, string currentDirectory, ref C024StartupInfo startup,
        out C024ProcessInformation processInformation);
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool LogonUserW(string user, string domain, string password,
        int logonType, int logonProvider, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError = true)]
    public static extern bool CreateRestrictedToken(IntPtr existingToken, uint flags,
        uint disableSidCount, IntPtr sidsToDisable, uint deletePrivilegeCount,
        IntPtr privilegesToDelete, uint restrictedSidCount, IntPtr sidsToRestrict,
        out IntPtr newToken);
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool ConvertStringSidToSidW(string stringSid, out IntPtr sid);
    [DllImport("advapi32.dll", SetLastError = true)]
    public static extern bool CreateProcessAsUserW(IntPtr token, string application,
        StringBuilder commandLine, IntPtr processAttributes, IntPtr threadAttributes,
        bool inheritHandles, uint creationFlags, IntPtr environment, IntPtr currentDirectory,
        ref C024StartupInfo startup, out C024ProcessInformation processInformation);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetInformationJobObject(IntPtr job, int infoClass,
        ref C024JobExtendedLimitInformation info, uint length);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool TerminateProcess(IntPtr process, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool TerminateJobObject(IntPtr job, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")]
    public static extern IntPtr LocalFree(IntPtr memory);
}
'@

function Set-FixtureAcl([string] $Path, [string[]] $Sids, [bool] $BroadRead = $false) {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    $full = [Security.AccessControl.FileSystemRights]::FullControl
    foreach ($sid in $Sids) {
        $sidReference = [Security.Principal.SecurityIdentifier]::new($sid)
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            $sidReference, $full,
            [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit,
            [Security.AccessControl.PropagationFlags]::None,
            [Security.AccessControl.AccessControlType]::Allow)
        [void]$acl.AddAccessRule($rule)
    }
    if ($BroadRead) {
        $read = [Security.AccessControl.FileSystemRights]::ReadAndExecute
        foreach ($sid in @('S-1-5-32-545', 'S-1-5-11', 'S-1-1-0')) {
            $sidReference = [Security.Principal.SecurityIdentifier]::new($sid)
            $rule = [Security.AccessControl.FileSystemAccessRule]::new(
                $sidReference, $read,
                [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit,
                [Security.AccessControl.PropagationFlags]::None,
                [Security.AccessControl.AccessControlType]::Allow)
            [void]$acl.AddAccessRule($rule)
        }
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
}

function Get-SddlFingerprint([string] $Path) {
    $acl = Get-Acl -LiteralPath $Path
    $sddl = $acl.GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::All)
    $bytes = [Text.Encoding]::UTF8.GetBytes($sddl)
    $hash = [Security.Cryptography.SHA256]::HashData($bytes)
    [ordered]@{ owner = $acl.Owner; sddl_sha256 = [Convert]::ToHexString($hash); sddl = $sddl }
}

function Start-TestChild([string] $Mode, [string] $User, [string] $Password, [IntPtr] $Token,
        [string] $Exe, [string] $Command, [string] $Directory) {
    $startup = [C024StartupInfo]::new()
    $startup.cb = [Runtime.InteropServices.Marshal]::SizeOf([type][C024StartupInfo])
    $pi = [C024ProcessInformation]::new()
    $line = [Text.StringBuilder]::new($Command)
    $created = $false
    if ($Mode -eq 'normal') {
        $created = [C024Native]::CreateProcessWithLogonW($User, $null, $Password, 1, $Exe,
            $line, 0x00000004, [IntPtr]::Zero, $Directory, [ref]$startup, [ref]$pi)
    } else {
        $created = [C024Native]::CreateProcessAsUserW($Token, $Exe, $line, [IntPtr]::Zero,
            [IntPtr]::Zero, $false, 0x00000004, [IntPtr]::Zero,
            [IntPtr]::Zero, [ref]$startup, [ref]$pi)
    }
    if (-not $created) {
        return [ordered]@{ created = $false; win32_error = [Runtime.InteropServices.Marshal]::GetLastWin32Error(); exit_code = $null; result = $null }
    }
    $resultPath = Join-Path $stage 'probe-result.json'
    Remove-Item -LiteralPath $resultPath -Force -ErrorAction SilentlyContinue
    $localJob = [C024Native]::CreateJobObjectW([IntPtr]::Zero, $null)
    if ($localJob -eq [IntPtr]::Zero) {
        [void][C024Native]::TerminateProcess($pi.hProcess, 1)
        [void][C024Native]::CloseHandle($pi.hThread)
        [void][C024Native]::CloseHandle($pi.hProcess)
        throw "CreateJobObjectW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    try {
        $limits = [C024JobExtendedLimitInformation]::new()
        $basic = $limits.BasicLimitInformation
        $basic.LimitFlags = 0x00002000
        $limits.BasicLimitInformation = $basic
        $size = [uint32][Runtime.InteropServices.Marshal]::SizeOf([type][C024JobExtendedLimitInformation])
        if (-not [C024Native]::SetInformationJobObject($localJob, 9, [ref]$limits, $size)) { throw "SetInformationJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
        if (-not [C024Native]::AssignProcessToJobObject($localJob, $pi.hProcess)) { throw "AssignProcessToJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
        if ([C024Native]::ResumeThread($pi.hThread) -eq [uint32]::MaxValue) { throw "ResumeThread failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
        $wait = [C024Native]::WaitForSingleObject($pi.hProcess, 60000)
        if ($wait -ne 0) { throw "C024 child wait failed: $wait" }
        $exitCode = [uint32]0
        [void][C024Native]::GetExitCodeProcess($pi.hProcess, [ref]$exitCode)
    } catch {
        [void][C024Native]::TerminateJobObject($localJob, 1)
        [void][C024Native]::TerminateProcess($pi.hProcess, 1)
        [void][C024Native]::WaitForSingleObject($pi.hProcess, 5000)
        throw
    } finally {
        [void][C024Native]::CloseHandle($localJob)
        [void][C024Native]::CloseHandle($pi.hThread)
        [void][C024Native]::CloseHandle($pi.hProcess)
    }
    $result = if (Test-Path -LiteralPath $resultPath) { Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json } else { $null }
    [ordered]@{ created = $true; win32_error = 0; exit_code = $exitCode; result = $result }
}

try {
    New-Item -ItemType Directory -Path $root | Out-Null
    New-Item -ItemType Directory -Path $stage, $broadRoot, $privateRoot | Out-Null
    $runnerSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $systemSid = 'S-1-5-18'
    $limitingSid = "S-1-5-21-2800000000-2800000001-2800000002-$([int](Get-Random -Minimum 1000 -Maximum 2000000000))"
    $passwordBytes = [byte[]]::new(36)
    [Security.Cryptography.RandomNumberGenerator]::Fill($passwordBytes)
    $password = [Convert]::ToBase64String($passwordBytes) + 'aA!9'
    [Array]::Clear($passwordBytes, 0, $passwordBytes.Length)
    $passwordSecure = ConvertTo-SecureString $password -AsPlainText -Force
    New-LocalUser -Name $account -Password $passwordSecure -AccountNeverExpires -PasswordNeverExpires `
        -Description 'Disposable CodeGG C024 feasibility principal' | Out-Null
    $accountCreated = $true
    $accountSid = (Get-LocalUser -Name $account).SID.Value
    Set-FixtureAcl -Path $stage -Sids @($runnerSid, $systemSid, $accountSid, $limitingSid)
    Set-FixtureAcl -Path $broadRoot -Sids @($runnerSid, $systemSid) -BroadRead $true
    Set-FixtureAcl -Path $privateRoot -Sids @($runnerSid, $systemSid)
    $broadSecret = Join-Path $broadRoot 'synthetic-broad-readable-secret.txt'
    $privateSecret = Join-Path $privateRoot 'synthetic-private-secret.txt'
    [IO.File]::WriteAllText($broadSecret, 'synthetic-c024-broad-secret')
    [IO.File]::WriteAllText($privateSecret, 'synthetic-c024-private-secret')
    $broadBefore = Get-SddlFingerprint $broadRoot
    $privateBefore = Get-SddlFingerprint $privateRoot
    $secretHashBefore = (Get-FileHash -LiteralPath $broadSecret -Algorithm SHA256).Hash
    $privateHashBefore = (Get-FileHash -LiteralPath $privateSecret -Algorithm SHA256).Hash

    $systemRoot = $env:SystemRoot
    $powerShell = Join-Path $systemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $encodedSecret = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($broadSecret))
    $encodedPrivate = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($privateSecret))
    $encodedStage = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($stage))
    $encodedLimitingSid = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($limitingSid))
    $scriptText = @"
`$ErrorActionPreference = 'Stop'
`$secret = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('$encodedSecret'))
`$private = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('$encodedPrivate'))
`$stage = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('$encodedStage'))
`$limitingSid = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('$encodedLimitingSid'))
`$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
`$tokenReport = (& (Join-Path `$env:SystemRoot 'System32\whoami.exe') /all 2>&1 | Out-String)
`$observed = [ordered]@{ token_user = `$identity.User.Value; normal_groups = @(`$identity.Groups | ForEach-Object { `$_.Value }); limiting_sid_seen_in_child = `$tokenReport.Contains(`$limitingSid); child_token_report = `$tokenReport; broad_direct = 'not_run'; private_control = 'not_run'; stage_write = 'not_run'; shell = 'running' }
try { `$observed.broad_direct = if ([IO.File]::ReadAllText(`$secret) -eq 'synthetic-c024-broad-secret') {'allowed'} else {'wrong_content'} } catch { `$observed.broad_direct = 'denied:' + `$_.Exception.GetType().Name }
try { [void][IO.File]::ReadAllText(`$private); `$observed.private_control = 'allowed' } catch { `$observed.private_control = 'denied:' + `$_.Exception.GetType().Name }
try { [IO.File]::WriteAllText([IO.Path]::Combine(`$stage, 'child-write.txt'), 'stage-ok'); `$observed.stage_write = 'allowed' } catch { `$observed.stage_write = 'denied:' + `$_.Exception.GetType().Name }
`$observed | ConvertTo-Json -Compress | Set-Content -LiteralPath ([IO.Path]::Combine(`$stage, 'probe-result.json')) -Encoding UTF8
"@
    $scriptPath = Join-Path $stage 'probe.ps1'
    [IO.File]::WriteAllText($scriptPath, $scriptText, [Text.Encoding]::Unicode)
    $arguments = "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$scriptPath`""
    $commandLine = "`"$powerShell`" $arguments"
    $normal = Start-TestChild -Mode normal -User $account -Password $password -Token ([IntPtr]::Zero) `
        -Exe $powerShell -Command $commandLine -Directory $stage
    if (-not $normal.created) { throw "Normal dedicated-account baseline could not launch: Win32 $($normal.win32_error)" }
    if (-not $normal.result -or $normal.result.token_user -ne $accountSid -or
        $normal.result.broad_direct -ne 'allowed' -or $normal.result.private_control -notlike 'denied:*') {
        throw "Normal-account broad/private baseline was invalid: $($normal | ConvertTo-Json -Compress -Depth 5)"
    }

    $domain = $env:COMPUTERNAME
    $plainPassword = $password
    if (-not [C024Native]::LogonUserW($account, $domain, $plainPassword, 2, 0, [ref]$baseToken)) {
        $logonError = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        if (-not [C024Native]::LogonUserW($account, $domain, $plainPassword, 4, 0, [ref]$baseToken)) {
            throw "Sandbox-account token acquisition failed: Win32 $([Runtime.InteropServices.Marshal]::GetLastWin32Error()) (interactive attempt $logonError)"
        }
    }
    $sidPointer = [IntPtr]::Zero
    $sidArray = [IntPtr]::Zero
    try {
        if (-not [C024Native]::ConvertStringSidToSidW($limitingSid, [ref]$sidPointer)) { throw "ConvertStringSidToSidW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
        $sidArray = [Runtime.InteropServices.Marshal]::AllocHGlobal([Runtime.InteropServices.Marshal]::SizeOf([type][IntPtr]) + 8)
        [Runtime.InteropServices.Marshal]::WriteIntPtr($sidArray, $sidPointer)
        # CreateRestrictedToken requires Attributes=0 for entries in SidsToRestrict.
        [Runtime.InteropServices.Marshal]::WriteInt32($sidArray, [Runtime.InteropServices.Marshal]::SizeOf([type][IntPtr]), 0)
        if (-not [C024Native]::CreateRestrictedToken($baseToken, 9, 0, [IntPtr]::Zero, 0, [IntPtr]::Zero, 1, $sidArray, [ref]$writeRestrictedToken)) {
            throw "CreateRestrictedToken(WRITE_RESTRICTED) failed: Win32 $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        }
        if (-not [C024Native]::CreateRestrictedToken($baseToken, 1, 0, [IntPtr]::Zero, 0, [IntPtr]::Zero, 1, $sidArray, [ref]$restrictedToken)) {
            throw "CreateRestrictedToken(read-restricted) failed: Win32 $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        }
    } finally {
        if ($sidArray -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::FreeHGlobal($sidArray) }
        if ($sidPointer -ne [IntPtr]::Zero) { [void][C024Native]::LocalFree($sidPointer) }
    }
    $writeRestricted = Start-TestChild -Mode write-restricted -User $account -Password $null -Token $writeRestrictedToken `
        -Exe $powerShell -Command $commandLine -Directory $null
    $restricted = Start-TestChild -Mode restricted -User $account -Password $null -Token $restrictedToken `
        -Exe $powerShell -Command $commandLine -Directory $null
    $broadAfter = Get-SddlFingerprint $broadRoot
    $privateAfter = Get-SddlFingerprint $privateRoot
    $secretHashAfter = (Get-FileHash -LiteralPath $broadSecret -Algorithm SHA256).Hash
    $privateHashAfter = (Get-FileHash -LiteralPath $privateSecret -Algorithm SHA256).Hash
    $os = Get-CimInstance Win32_OperatingSystem
    $version = Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
    $strictPositive = $restricted.created -and $restricted.result -and
        $restricted.result.token_user -eq $accountSid -and
        $restricted.result.limiting_sid_seen_in_child -and
        $restricted.result.broad_direct -like 'denied:*' -and
        $restricted.result.stage_write -eq 'allowed'
    $disposition = if ($strictPositive) { 'partial-positive-matrix-incomplete' } else { 'negative-or-blocked-tool-launch' }
    $report = [ordered]@{
        contract = 'codegg.windows_dedicated_user_read_boundary.v1'
        disposition = $disposition
        source_sha = (git rev-parse HEAD).Trim()
        os = $os.Caption
        version = $os.Version
        build = $version.CurrentBuild
        ubr = $version.UBR
        runner_identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
        account_sid = $accountSid
        restricting_sid = $limitingSid
        normal_account_baseline = $normal
        write_restricted_child = $writeRestricted
        restricted_primary_token_launch = $restricted
        broad_fixture_sddl_before = $broadBefore
        broad_fixture_sddl_after = $broadAfter
        private_fixture_sddl_before = $privateBefore
        private_fixture_sddl_after = $privateAfter
        broad_secret_sha256_unchanged = ($secretHashBefore -eq $secretHashAfter)
        private_secret_sha256_unchanged = ($privateHashBefore -eq $privateHashAfter)
        host_acl_mutation = $false
        write_restricted_read_semantics = if ($writeRestricted.result) { $writeRestricted.result.broad_direct } else { 'unavailable' }
        aliases_ipc_inherited_handle_tools_descendants = 'not run; this bounded token/launch comparison does not claim a strict-positive matrix'
        production_admission = $false
        network_isolation = 'not tested and not claimed'
    }
    $json = $report | ConvertTo-Json -Depth 8 -Compress
    "C024_REPORT=$json"
    $json | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY
    if ($broadBefore.sddl_sha256 -ne $broadAfter.sddl_sha256 -or
        $privateBefore.sddl_sha256 -ne $privateAfter.sddl_sha256 -or
        -not $report.broad_secret_sha256_unchanged -or -not $report.private_secret_sha256_unchanged) {
        throw 'C024 changed a source fixture descriptor or content hash.'
    }
    if ($restricted.result -and $restricted.result.broad_direct -eq 'allowed') {
        throw 'C024 security-negative: the true restricting-SID child read the broad-readable source fixture.'
    }
} catch {
    Write-Output "C024_FAILURE=$($_.Exception.ToString())"
    throw
} finally {
    if ($job -ne [IntPtr]::Zero) { [void][C024Native]::CloseHandle($job) }
    if ($restrictedToken -ne [IntPtr]::Zero) { [void][C024Native]::CloseHandle($restrictedToken) }
    if ($writeRestrictedToken -ne [IntPtr]::Zero) { [void][C024Native]::CloseHandle($writeRestrictedToken) }
    if ($baseToken -ne [IntPtr]::Zero) { [void][C024Native]::CloseHandle($baseToken) }
    if ($accountCreated) {
        Remove-LocalUser -Name $account -ErrorAction SilentlyContinue
        if (-not (Get-LocalUser -Name $account -ErrorAction SilentlyContinue)) { Write-Output 'C024_CLEANUP principal_removed=true' }
    }
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue }
    if ($passwordSecure) { $passwordSecure.Dispose() }
}
