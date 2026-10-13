$ErrorActionPreference = 'Stop'

# Test-only C032 diagnostic. This deliberately creates one disposable local user
# and confines all writable fixtures to RUNNER_TEMP. It never edits an existing
# workspace/profile ACL or installs a service.
$runId = [Guid]::NewGuid().ToString('N')
$account = "c032_$($runId.Substring(0, 8))"
$root = Join-Path $env:RUNNER_TEMP "codegg-c032-$runId"
$immutable = Join-Path $root 'immutable'
$stage = Join-Path $root 'stage'
$hostRoot = Join-Path $root 'broad-readable-host'
$siblingRoot = Join-Path $root 'broad-readable-sibling'
$secret = Join-Path $hostRoot 'synthetic-c032-secret.txt'
$siblingSecret = Join-Path $siblingRoot 'synthetic-c032-sibling.txt'
$junction = Join-Path $root 'host-junction'
$hardlink = Join-Path $root 'synthetic-c032-hardlink.txt'
$ads = "$secret`:c032"
$runnerScript = Join-Path $immutable 'two-hop-runner.ps1'
$reportPath = Join-Path $stage 'report.json'
$principalCreated = $false
$profileRemoved = $true
$job = [IntPtr]::Zero
$processInfo = $null

$nativeSource = @'
using System;
using System.Runtime.InteropServices;
using System.Text;

[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct C032StartupInfo {
    public int cb;
    public string lpReserved, lpDesktop, lpTitle;
    public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars;
    public int dwFillAttribute, dwFlags;
    public short wShowWindow, cbReserved2;
    public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
}
[StructLayout(LayoutKind.Sequential)]
public struct C032ProcessInformation {
    public IntPtr hProcess, hThread;
    public uint dwProcessId, dwThreadId;
}
[StructLayout(LayoutKind.Sequential)]
public struct C032JobBasicLimitInformation {
    public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
    public uint LimitFlags;
    public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
    public uint ActiveProcessLimit;
    public UIntPtr Affinity;
    public uint PriorityClass, SchedulingClass;
}
[StructLayout(LayoutKind.Sequential)]
public struct C032IoCounters {
    public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
    public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
}
[StructLayout(LayoutKind.Sequential)]
public struct C032JobExtendedLimitInformation {
    public C032JobBasicLimitInformation BasicLimitInformation;
    public C032IoCounters IoInfo;
    public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
}
public static class C032Native {
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern bool CreateProcessWithLogonW(string user, string domain, string password,
        int logonFlags, string application, StringBuilder commandLine, int creationFlags,
        IntPtr environment, string currentDirectory, ref C032StartupInfo startup,
        out C032ProcessInformation processInformation);
    [DllImport("advapi32.dll", SetLastError=true)]
    public static extern bool CreateRestrictedToken(IntPtr existingToken, uint flags,
        uint disableSidCount, IntPtr sidsToDisable, uint deletePrivilegeCount,
        IntPtr privilegesToDelete, uint restrictedSidCount, IntPtr sidsToRestrict,
        out IntPtr newToken);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern bool ConvertStringSidToSidW(string sid, out IntPtr result);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern bool ConvertSidToStringSidW(IntPtr sid, out IntPtr result);
    [DllImport("advapi32.dll", SetLastError=true)]
    public static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("kernel32.dll")]
    public static extern IntPtr GetCurrentProcess();
    [DllImport("advapi32.dll", SetLastError=true)]
    public static extern bool GetTokenInformation(IntPtr token, int infoClass, IntPtr buffer,
        int bufferLength, out int returnLength);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern bool CreateProcessAsUserW(IntPtr token, string application,
        StringBuilder commandLine, IntPtr processAttributes, IntPtr threadAttributes,
        bool inheritHandles, uint creationFlags, IntPtr environment, string currentDirectory,
        ref C032StartupInfo startup, out C032ProcessInformation processInformation);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool SetInformationJobObject(IntPtr job, int infoClass,
        ref C032JobExtendedLimitInformation info, uint length);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool TerminateJobObject(IntPtr job, uint exitCode);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")] public static extern IntPtr LocalFree(IntPtr memory);
    public static string[] ReadRestrictedSids(IntPtr token) {
        int needed = 0;
        GetTokenInformation(token, 11, IntPtr.Zero, 0, out needed);
        if (needed <= 0) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        IntPtr buffer = Marshal.AllocHGlobal(needed);
        try {
            if (!GetTokenInformation(token, 11, buffer, needed, out needed))
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            int count = Marshal.ReadInt32(buffer);
            int offset = IntPtr.Size == 8 ? 8 : 4;
            int stride = IntPtr.Size == 8 ? 16 : 8;
            string[] values = new string[count];
            for (int i = 0; i < count; i++) {
                IntPtr sid = Marshal.ReadIntPtr(buffer, offset + i * stride);
                IntPtr text = IntPtr.Zero;
                if (!ConvertSidToStringSidW(sid, out text))
                    throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
                try { values[i] = Marshal.PtrToStringUni(text); }
                finally { LocalFree(text); }
            }
            return values;
        } finally { Marshal.FreeHGlobal(buffer); }
    }
    public static int ReadTokenType(IntPtr token) {
        IntPtr buffer = Marshal.AllocHGlobal(sizeof(int));
        try {
            int needed;
            if (!GetTokenInformation(token, 8, buffer, sizeof(int), out needed))
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            return Marshal.ReadInt32(buffer);
        } finally { Marshal.FreeHGlobal(buffer); }
    }
}
'@
Add-Type -TypeDefinition $nativeSource

function Set-ExplicitAcl([string] $Path, [hashtable] $Entries) {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($entry in $Entries.GetEnumerator()) {
        $sid = [Security.Principal.SecurityIdentifier]::new($entry.Key)
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            $sid, [Security.AccessControl.FileSystemRights]$entry.Value,
            [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit,
            [Security.AccessControl.PropagationFlags]::None,
            [Security.AccessControl.AccessControlType]::Allow)
        [void]$acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
}

function Set-ExplicitFileAcl([string] $Path, [string] $OwnerSid, [string] $SystemSid, [string] $BroadSid) {
    $acl = [Security.AccessControl.FileSecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($entry in @(@($OwnerSid, 'FullControl'), @($SystemSid, 'FullControl'), @($BroadSid, 'Read'))) {
        $sid = [Security.Principal.SecurityIdentifier]::new([string]$entry[0])
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            $sid, [Security.AccessControl.FileSystemRights]$entry[1],
            [Security.AccessControl.AccessControlType]::Allow)
        [void]$acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
}

try {
    New-Item -ItemType Directory -Path $immutable, $stage, $hostRoot, $siblingRoot | Out-Null
    $runner = [Security.Principal.WindowsIdentity]::GetCurrent()
    $runnerSid = $runner.User.Value
    $systemSid = 'S-1-5-18'
    $admin = [Security.Principal.WindowsPrincipal]::new($runner)
    if (-not $admin.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'C032 requires an administrator-consented disposable Windows fixture.'
    }

    $random = [byte[]]::new(32)
    [Security.Cryptography.RandomNumberGenerator]::Fill($random)
    $password = [Convert]::ToBase64String($random) + 'aA!9'
    [Array]::Clear($random, 0, $random.Length)
    $securePassword = ConvertTo-SecureString $password -AsPlainText -Force
    New-LocalUser -Name $account -Password $securePassword -AccountNeverExpires -PasswordNeverExpires `
        -Description 'Disposable CodeGG C032 feasibility principal' | Out-Null
    $principalCreated = $true
    $accountSid = (Get-LocalUser -Name $account).SID.Value
    $password = $null
    $limitingSid = "S-1-5-21-2800000000-2800000001-2800000002-$([int](Get-Random -Minimum 1000 -Maximum 2000000000))"

    Set-ExplicitAcl $root @{
        $runnerSid = 'FullControl'; $systemSid = 'FullControl';
        $accountSid = 'ReadAndExecute'; $limitingSid = 'ReadAndExecute'
    }
    Set-ExplicitAcl $stage @{
        $runnerSid = 'FullControl'; $systemSid = 'FullControl'; $accountSid = 'FullControl'; $limitingSid = 'FullControl'
    }
    Set-ExplicitAcl $immutable @{
        $runnerSid = 'FullControl'; $systemSid = 'FullControl';
        $accountSid = 'ReadAndExecute'; $limitingSid = 'ReadAndExecute'
    }
    Set-ExplicitAcl $hostRoot @{
        $runnerSid = 'FullControl'; $systemSid = 'FullControl'; 'S-1-1-0' = 'ReadAndExecute'
    }
    Set-ExplicitAcl $siblingRoot @{
        $runnerSid = 'FullControl'; $systemSid = 'FullControl'; 'S-1-1-0' = 'ReadAndExecute'
    }
    [IO.File]::WriteAllText($secret, 'synthetic-c032-broad-readable-secret')
    [IO.File]::WriteAllText($siblingSecret, 'synthetic-c032-broad-readable-sibling')
    [IO.File]::WriteAllText($ads, 'synthetic-c032-ads')
    Set-ExplicitFileAcl $secret $runnerSid $systemSid 'S-1-1-0'
    Set-ExplicitFileAcl $siblingSecret $runnerSid $systemSid 'S-1-1-0'
    $cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
    $junctionCommand = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($cmd, "/d /c mklink /J `"$junction`" `"$hostRoot`""))
    if (-not $junctionCommand.WaitForExit(15000) -or $junctionCommand.ExitCode -ne 0) { throw 'Could not create synthetic host junction.' }
    $hardlinkCommand = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($cmd, "/d /c mklink /H `"$hardlink`" `"$secret`""))
    if (-not $hardlinkCommand.WaitForExit(15000) -or $hardlinkCommand.ExitCode -ne 0) { throw 'Could not create synthetic hardlink alias.' }

    $script = @'
param([string]$Mode, [string]$Secret, [string]$Stage, [string]$Report, [string]$RestrictingSid, [string]$Git, [string]$SiblingSecret, [string]$JunctionSecret, [string]$HardlinkSecret, [string]$AdsPath)
$ErrorActionPreference = 'Stop'
if ($Mode -eq 'child') {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $processToken = [IntPtr]::Zero
    if (-not [C032Native]::OpenProcessToken([C032Native]::GetCurrentProcess(), 0x0008, [ref]$processToken)) {
        throw "OpenProcessToken failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    try {
        $restrictedSids = @([C032Native]::ReadRestrictedSids($processToken))
        $tokenType = [C032Native]::ReadTokenType($processToken)
    } finally { [void][C032Native]::CloseHandle($processToken) }
    $result = [ordered]@{
        mode = 'restricted-child'; child_sid = $identity.User.Value
        restricting_sid_requested = $RestrictingSid; restricting_sids_observed = $restrictedSids
        token_type = if ($tokenType -eq 1) { 'primary' } else { "other:$tokenType" }
        restricted_sid_attested = ($restrictedSids -contains $RestrictingSid)
    }
    foreach ($probe in @(
        @{ name = 'direct'; path = $Secret },
        @{ name = 'sibling'; path = $SiblingSecret },
        @{ name = 'junction'; path = $JunctionSecret },
        @{ name = 'hardlink'; path = $HardlinkSecret },
        @{ name = 'ads'; path = $AdsPath },
        @{ name = 'case_alias'; path = $Secret.ToUpperInvariant() },
        @{ name = 'extended_path'; path = "\\?\$Secret" }
    )) {
        try { [void][IO.File]::ReadAllText($probe.path); $result["read_$($probe.name)"] = 'allowed' }
        catch { $result["read_$($probe.name)"] = 'denied'; $result["read_$($probe.name)_error"] = $_.Exception.GetType().Name }
    }
    try { [IO.File]::WriteAllText((Join-Path $Stage 'stage-write.txt'), 'stage-ok'); $result.stage_write = 'allowed' }
    catch { $result.stage_write = 'failed'; $result.stage_error = $_.Exception.GetType().Name }
    $cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
    try {
        $p = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($cmd, '/d /c echo c032-shell-ok') )
        if ($p.WaitForExit(15000) -and $p.ExitCode -eq 0) { $result.native_shell = 'allowed' } else { $result.native_shell = 'failed' }
    } catch { $result.native_shell = 'failed'; $result.shell_error = $_.Exception.GetType().Name }
    try {
        $repo = Join-Path $Stage 'git-repo'
        $p = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($Git, "init --quiet `"$repo`""))
        if ($p.WaitForExit(20000) -and $p.ExitCode -eq 0) {
            $q = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($Git, "-C `"$repo`" status --porcelain"))
            $result.git = if ($q.WaitForExit(20000) -and $q.ExitCode -eq 0) { 'allowed' } else { 'failed' }
        } else { $result.git = 'failed' }
    } catch { $result.git = 'failed'; $result.git_error = $_.Exception.GetType().Name }
    $result.network = 'not_tested'
    $result.ipc = 'not_run'
    $result.unc = 'not_run'
    $result.aliases = 'direct,sibling,junction,hardlink,ads,case_alias,extended_path'
    $result.descendants = 'not_run'
    $result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Report -Encoding utf8
    exit 0
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$normalRead = 'failed'
try { $normalRead = if ([IO.File]::ReadAllText($Secret) -eq 'synthetic-c032-broad-readable-secret') { 'allowed' } else { 'wrong_content' } } catch { $normalRead = 'failed:' + $_.Exception.GetType().Name }
$sidPointer = [IntPtr]::Zero
$sidAttributes = [IntPtr]::Zero
$token = [IntPtr]::Zero
$child = [C032ProcessInformation]::new()
$created = $false
$errorCode = 0
    $innerSuspendCount = 0
try {
    if (-not [C032Native]::ConvertStringSidToSidW($RestrictingSid, [ref]$sidPointer)) { throw "ConvertStringSidToSidW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    $sidAttributes = [Runtime.InteropServices.Marshal]::AllocHGlobal([IntPtr]::Size + 8)
    [Runtime.InteropServices.Marshal]::WriteIntPtr($sidAttributes, $sidPointer)
    [Runtime.InteropServices.Marshal]::WriteInt32($sidAttributes, [IntPtr]::Size, 0)
    $currentToken = $identity.Token
    if (-not [C032Native]::CreateRestrictedToken($currentToken, 0, 0, [IntPtr]::Zero, 0, [IntPtr]::Zero, 1, $sidAttributes, [ref]$token)) {
        throw "CreateRestrictedToken failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    $pwsh = (Get-Process -Id $PID).Path
    $childArguments = @($PSCommandPath, 'child', $Secret, $Stage, $Report, $RestrictingSid, $Git, $SiblingSecret, $JunctionSecret, $HardlinkSecret, $AdsPath)
    $childCommand = '"' + $pwsh + '" -NoLogo -NoProfile -NonInteractive -File ' + (($childArguments | ForEach-Object { '"' + $_ + '"' }) -join ' ')
    $startup = [C032StartupInfo]::new(); $startup.cb = [Runtime.InteropServices.Marshal]::SizeOf([type][C032StartupInfo])
    $commandLine = [Text.StringBuilder]::new($childCommand)
    $created = [C032Native]::CreateProcessAsUserW($token, $pwsh, $commandLine, [IntPtr]::Zero, [IntPtr]::Zero, $false, 0x00000004, [IntPtr]::Zero, $Stage, [ref]$startup, [ref]$child)
    if (-not $created) { $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error(); throw "CreateProcessAsUserW failed: $errorCode" }
    $inJob = $false
    if (-not [C032Native]::IsProcessInJob($child.hProcess, [IntPtr]::Zero, [ref]$inJob) -or -not $inJob) { throw 'Restricted child did not inherit Job ownership.' }
    $innerSuspendCount = [C032Native]::ResumeThread($child.hThread)
    if ($innerSuspendCount -ne 1) { throw "Inner child suspend count was unexpected: $innerSuspendCount" }
    $wait = [C032Native]::WaitForSingleObject($child.hProcess, 90000)
    if ($wait -ne 0) { throw "Restricted child timed out (wait=$wait)." }
    $childResult = Get-Content -LiteralPath $Report -Raw | ConvertFrom-Json
    $childResult | Add-Member -NotePropertyName inner_child_suspend_count -NotePropertyValue $innerSuspendCount -Force
    $childResult | Add-Member -NotePropertyName inner_child_owned_job_verified_before_resume -NotePropertyValue $true -Force
    $childResult | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Report -Encoding utf8
} catch {
    if ($created) { $errorCode = $errorCode -or [Runtime.InteropServices.Marshal]::GetLastWin32Error() }
    [ordered]@{ contract = 'codegg.windows_two_hop_feasibility.v1'; phase = 'runner'; normal_account_same_file_read = $normalRead; runner_sid = $identity.User.Value; restricting_sid = $RestrictingSid; launch_error = $errorCode; launch_failure = $_.Exception.Message; child_reached_entry = $false; network = 'not_tested'; production_admission = 'not_authorized' } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Report -Encoding utf8
} finally {
    if ($child.hThread -ne [IntPtr]::Zero) { [void][C032Native]::CloseHandle($child.hThread) }
    if ($child.hProcess -ne [IntPtr]::Zero) { [void][C032Native]::CloseHandle($child.hProcess) }
    if ($token -ne [IntPtr]::Zero) { [void][C032Native]::CloseHandle($token) }
    if ($sidAttributes -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::FreeHGlobal($sidAttributes) }
    if ($sidPointer -ne [IntPtr]::Zero) { [void][C032Native]::LocalFree($sidPointer) }
}
'@
    $script = "Add-Type -TypeDefinition @'`n$nativeSource`n'@`n" + $script
    [IO.File]::WriteAllText($runnerScript, $script, [Text.Encoding]::Unicode)
    $runnerAcl = Get-Acl -LiteralPath $runnerScript
    $runnerAcl.SetAccessRuleProtection($true, $false)
    foreach ($entry in @(@($runnerSid, 'FullControl'), @($systemSid, 'FullControl'), @($accountSid, 'ReadAndExecute'))) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            [Security.Principal.SecurityIdentifier]::new([string]$entry[0]),
            [Security.AccessControl.FileSystemRights]$entry[1],
            [Security.AccessControl.AccessControlType]::Allow)
        [void]$runnerAcl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $runnerScript -AclObject $runnerAcl
    $scriptHash = (Get-FileHash -LiteralPath $runnerScript -Algorithm SHA256).Hash
    $hostDaclBefore = (Get-Acl -LiteralPath $hostRoot).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::All)
    $secretDaclBefore = (Get-Acl -LiteralPath $secret).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::All)
    $secretHashBefore = (Get-FileHash -LiteralPath $secret -Algorithm SHA256).Hash
    $stageDaclBefore = (Get-Acl -LiteralPath $stage).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::All)
    $git = (Get-Command git.exe -ErrorAction Stop).Source
    $pwsh = (Get-Process -Id $PID).Path
    $passwordBytes = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($securePassword)
    try { $plainPassword = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($passwordBytes) }
    finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($passwordBytes) }
    $args = @($runnerScript, 'runner', $secret, $stage, $reportPath, $limitingSid, $git, $siblingSecret, (Join-Path $junction 'synthetic-c032-secret.txt'), $hardlink, $ads) | ForEach-Object { '"' + $_.Replace('"','\"') + '"' }
    $line = [Text.StringBuilder]::new('"' + $pwsh + '" -NoLogo -NoProfile -NonInteractive -File ' + ($args -join ' '))
    $startup = [C032StartupInfo]::new(); $startup.cb = [Runtime.InteropServices.Marshal]::SizeOf([type][C032StartupInfo])
    $processInfo = [C032ProcessInformation]::new()
    $environmentEntries = @(
        "PATH=$env:SystemRoot\System32",
        "SystemDrive=$env:SystemDrive",
        "SystemRoot=$env:SystemRoot",
        "TEMP=$stage",
        "TMP=$stage",
        "USERPROFILE=$env:SystemDrive\Users\$account",
        "USERNAME=$account",
        "WINDIR=$env:SystemRoot"
    ) | Sort-Object
    $environmentBlock = [Runtime.InteropServices.Marshal]::StringToHGlobalUni(($environmentEntries -join "`0") + "`0`0")
    $created = [C032Native]::CreateProcessWithLogonW($account, $null, $plainPassword, 1, $pwsh, $line, 0x00000404, $environmentBlock, $stage, [ref]$startup, [ref]$processInfo)
    [Runtime.InteropServices.Marshal]::FreeHGlobal($environmentBlock)
    $plainPassword = $null
    if (-not $created) { throw "CreateProcessWithLogonW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    $job = [C032Native]::CreateJobObjectW([IntPtr]::Zero, $null)
    if ($job -eq [IntPtr]::Zero) { throw "CreateJobObjectW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    $limits = [C032JobExtendedLimitInformation]::new(); $basic = $limits.BasicLimitInformation; $basic.LimitFlags = 0x00002000; $limits.BasicLimitInformation = $basic
    $size = [uint32][Runtime.InteropServices.Marshal]::SizeOf([type][C032JobExtendedLimitInformation])
    if (-not [C032Native]::SetInformationJobObject($job, 9, [ref]$limits, $size)) { throw "SetInformationJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    if (-not [C032Native]::AssignProcessToJobObject($job, $processInfo.hProcess)) { throw "AssignProcessToJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    $runnerInJob = $false
    if (-not [C032Native]::IsProcessInJob($processInfo.hProcess, $job, [ref]$runnerInJob) -or -not $runnerInJob) { throw 'First-hop runner was not verified in the owned Job before resume.' }
    $outerSuspendCount = [C032Native]::ResumeThread($processInfo.hThread)
    if ($outerSuspendCount -ne 1) { throw "First-hop runner suspend count was unexpected: $outerSuspendCount" }
    $wait = [C032Native]::WaitForSingleObject($processInfo.hProcess, 120000)
    if ($wait -ne 0) { [void][C032Native]::TerminateJobObject($job, 1); throw "First-hop runner did not exit (wait=$wait)." }
    if (-not (Test-Path -LiteralPath $reportPath)) { throw 'Two-hop runner produced no result report.' }
    $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    $report | Add-Member -NotePropertyName contract -NotePropertyValue 'codegg.windows_two_hop_feasibility.v1' -Force
    $report | Add-Member -NotePropertyName child_reached_entry -NotePropertyValue ($report.mode -eq 'restricted-child') -Force
    $report | Add-Member -NotePropertyName host_edition -NotePropertyValue (Get-CimInstance Win32_OperatingSystem).Caption
    $report | Add-Member -NotePropertyName host_build -NotePropertyValue (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuild
    $report | Add-Member -NotePropertyName runner_executable -NotePropertyValue $pwsh
    $report | Add-Member -NotePropertyName runner_executable_sha256 -NotePropertyValue (Get-FileHash -LiteralPath $pwsh -Algorithm SHA256).Hash
    $report | Add-Member -NotePropertyName immutable_runner_script_sha256 -NotePropertyValue $scriptHash
    $report | Add-Member -NotePropertyName first_hop_suspend_count -NotePropertyValue $outerSuspendCount
    $report | Add-Member -NotePropertyName first_hop_owned_job_verified_before_resume -NotePropertyValue $runnerInJob
    $report | Add-Member -NotePropertyName host_fixture_dacl_unchanged -NotePropertyValue ($hostDaclBefore -eq (Get-Acl -LiteralPath $hostRoot).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::All))
    $report | Add-Member -NotePropertyName secret_dacl_unchanged -NotePropertyValue ($secretDaclBefore -eq (Get-Acl -LiteralPath $secret).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::All))
    $report | Add-Member -NotePropertyName secret_sha256_unchanged -NotePropertyValue ($secretHashBefore -eq (Get-FileHash -LiteralPath $secret -Algorithm SHA256).Hash)
    $report | Add-Member -NotePropertyName stage_dacl_unchanged -NotePropertyValue ($stageDaclBefore -eq (Get-Acl -LiteralPath $stage).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::All))
    $report | Add-Member -NotePropertyName original_fixture_metadata_changed -NotePropertyValue $false
    $report | Add-Member -NotePropertyName account_profile_teardown -NotePropertyValue 'deferred to final cleanup; see C032_CLEANUP'
    $report | Add-Member -NotePropertyName qualification -NotePropertyValue 'Hosted disposable-runner diagnostic only; not an independent security admission.'
    $json = $report | ConvertTo-Json -Depth 8 -Compress
    "C032_REPORT=$json"
    if ($env:GITHUB_STEP_SUMMARY) { $json | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY }
} finally {
    if ($job -ne [IntPtr]::Zero) { [void][C032Native]::CloseHandle($job) }
    if ($processInfo -and $processInfo.hThread -ne [IntPtr]::Zero) { [void][C032Native]::CloseHandle($processInfo.hThread) }
    if ($processInfo -and $processInfo.hProcess -ne [IntPtr]::Zero) { [void][C032Native]::CloseHandle($processInfo.hProcess) }
    if ($principalCreated) {
        $profile = Get-CimInstance Win32_UserProfile -ErrorAction SilentlyContinue |
            Where-Object { $_.SID -eq $accountSid -and $_.LocalPath -eq (Join-Path $env:SystemDrive "Users\$account") }
        if ($profile) {
            Remove-CimInstance -InputObject $profile -ErrorAction SilentlyContinue
            $profileRemoved = -not (Get-CimInstance Win32_UserProfile -ErrorAction SilentlyContinue | Where-Object { $_.SID -eq $accountSid })
        }
        Remove-LocalUser -Name $account -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue }
    if ($securePassword) { $securePassword.Dispose() }
    Write-Output "C032_CLEANUP principal_removed=$(-not (Get-LocalUser -Name $account -ErrorAction SilentlyContinue)) profile_removed=$profileRemoved fixtures_removed=$(-not (Test-Path -LiteralPath $root))"
}
