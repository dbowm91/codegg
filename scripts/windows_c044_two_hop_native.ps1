$ErrorActionPreference = 'Stop'

# Test-only C044 diagnostic: compiled native inner child instead of the
# historical C032 PowerShell inner child (C032 closed negative: the
# PowerShell child never reached entry). One disposable local user, all
# fixtures confined to RUNNER_TEMP. Never edits an existing
# workspace/profile ACL, never installs a service or persistent account,
# never enables production SandboxRequest::Required.
#
# Inputs (environment):
#   C044_NATIVE_PROBE  Absolute path to the cl.exe-built native_probe.exe
#                      (built + hashed by the calling workflow).
#   C044_PROBE_SHA256  Expected SHA-256 of that binary (fail if mismatch).
#   C044_PROBE_COMPILER Recorded cl.exe provenance string (recorded only).
$runId = [Guid]::NewGuid().ToString('N')
$nonce = $runId
$account = "c044_$($runId.Substring(0, 8))"
$root = Join-Path $env:RUNNER_TEMP "codegg-c044-$runId"
$immutable = Join-Path $root 'immutable'
$stage = Join-Path $root 'stage'
$hostRoot = Join-Path $root 'broad-readable-host'
$secret = Join-Path $hostRoot 'synthetic-c044-secret.txt'
$junction = Join-Path $root 'host-junction'
$hardlink = Join-Path $root 'synthetic-c044-hardlink.txt'
$ads = "$secret`:c044"
$hostWritePath = Join-Path $hostRoot 'c044-host-write-attempt.bin'
$runnerScript = Join-Path $immutable 'two-hop-runner.ps1'
$probeStaged = Join-Path $immutable 'native_probe.exe'
$reportPath = Join-Path $stage 'report.json'
$entryPath = Join-Path $stage "entry-$nonce.json"
$proofPath = Join-Path $stage "proof-$nonce.json"
$principalCreated = $false
$profileRemoved = $true
$job = [IntPtr]::Zero
$processInfo = $null

$nativeSource = @'
using System;
using System.Runtime.InteropServices;
using System.Text;

[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct C044StartupInfo {
    public int cb;
    public string lpReserved, lpDesktop, lpTitle;
    public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars;
    public int dwFillAttribute, dwFlags;
    public short wShowWindow, cbReserved2;
    public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
}
[StructLayout(LayoutKind.Sequential)]
public struct C044ProcessInformation {
    public IntPtr hProcess, hThread;
    public uint dwProcessId, dwThreadId;
}
[StructLayout(LayoutKind.Sequential)]
public struct C044JobBasicLimitInformation {
    public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
    public uint LimitFlags;
    public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
    public uint ActiveProcessLimit;
    public UIntPtr Affinity;
    public uint PriorityClass, SchedulingClass;
}
[StructLayout(LayoutKind.Sequential)]
public struct C044IoCounters {
    public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
    public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
}
[StructLayout(LayoutKind.Sequential)]
public struct C044JobExtendedLimitInformation {
    public C044JobBasicLimitInformation BasicLimitInformation;
    public C044IoCounters IoInfo;
    public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
}
public static class C044Native {
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern bool CreateProcessWithLogonW(string user, string domain, string password,
        int logonFlags, string application, StringBuilder commandLine, int creationFlags,
        IntPtr environment, string currentDirectory, ref C044StartupInfo startup,
        out C044ProcessInformation processInformation);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern bool LogonUserW(string username, string domain, string password,
        int logonType, int logonProvider, out IntPtr token);
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
        ref C044StartupInfo startup, out C044ProcessInformation processInformation);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool SetInformationJobObject(IntPtr job, int infoClass,
        ref C044JobExtendedLimitInformation info, uint length);
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
    if ([string]::IsNullOrWhiteSpace($env:C044_NATIVE_PROBE) -or -not (Test-Path -LiteralPath $env:C044_NATIVE_PROBE)) {
        throw 'C044 requires the workflow-built native_probe.exe via C044_NATIVE_PROBE.'
    }
    if ([string]::IsNullOrWhiteSpace($env:C044_PROBE_SHA256)) {
        throw 'C044 requires the expected probe hash via C044_PROBE_SHA256.'
    }
    New-Item -ItemType Directory -Path $immutable, $stage, $hostRoot | Out-Null
    $runner = [Security.Principal.WindowsIdentity]::GetCurrent()
    $runnerSid = $runner.User.Value
    $systemSid = 'S-1-5-18'
    $admin = [Security.Principal.WindowsPrincipal]::new($runner)
    if (-not $admin.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'C044 requires an administrator-consented disposable Windows fixture.'
    }

    $random = [byte[]]::new(32)
    [Security.Cryptography.RandomNumberGenerator]::Fill($random)
    $password = [Convert]::ToBase64String($random) + 'aA!9'
    [Array]::Clear($random, 0, $random.Length)
    $securePassword = ConvertTo-SecureString $password -AsPlainText -Force
    New-LocalUser -Name $account -Password $securePassword -AccountNeverExpires -PasswordNeverExpires `
        -Description 'Disposable CodeGG C044 feasibility principal' | Out-Null
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
    [IO.File]::WriteAllText($secret, 'synthetic-c044-broad-readable-secret')
    [IO.File]::WriteAllText($ads, 'synthetic-c044-ads')
    Set-ExplicitFileAcl $secret $runnerSid $systemSid 'S-1-1-0'
    $cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
    $junctionCommand = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($cmd, "/d /c mklink /J `"$junction`" `"$hostRoot`""))
    if (-not $junctionCommand.WaitForExit(15000) -or $junctionCommand.ExitCode -ne 0) { throw 'Could not create synthetic host junction.' }
    $hardlinkCommand = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($cmd, "/d /c mklink /H `"$hardlink`" `"$secret`""))
    if (-not $hardlinkCommand.WaitForExit(15000) -or $hardlinkCommand.ExitCode -ne 0) { throw 'Could not create synthetic hardlink alias.' }

    Copy-Item -LiteralPath $env:C044_NATIVE_PROBE -Destination $probeStaged -Force
    $stagedHash = (Get-FileHash -LiteralPath $probeStaged -Algorithm SHA256).Hash
    if ($stagedHash -ne $env:C044_PROBE_SHA256) { throw 'Staged native probe hash does not match the workflow-pinned hash.' }
    $probeAcl = Get-Acl -LiteralPath $probeStaged
    $probeAcl.SetAccessRuleProtection($true, $false)
    foreach ($entry in @(@($runnerSid, 'FullControl'), @($systemSid, 'FullControl'), @($accountSid, 'ReadAndExecute'))) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            [Security.Principal.SecurityIdentifier]::new([string]$entry[0]),
            [Security.AccessControl.FileSystemRights]$entry[1],
            [Security.AccessControl.AccessControlType]::Allow)
        [void]$probeAcl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $probeStaged -AclObject $probeAcl

    $junctionSecret = Join-Path $junction 'synthetic-c044-secret.txt'
    # Alias routes travel via a stage file, not argv: keeps the first-hop
    # LogonW command line short and free of exotic characters.
    $aliasFile = Join-Path $stage 'aliases.txt'
    @($junctionSecret, $hardlink, $ads, $secret.ToUpperInvariant(), "\\?\$secret") | Set-Content -LiteralPath $aliasFile -Encoding ascii

    $script = @'
param([string]$Mode, [string]$ParamsFile)
$ErrorActionPreference = 'Stop'
$params = @{}
foreach ($paramLine in @(Get-Content -LiteralPath $ParamsFile -Encoding ascii)) {
    if ([string]::IsNullOrWhiteSpace($paramLine)) { continue }
    $k, $v = $paramLine -split '=', 2
    $params[$k.Trim()] = $v
}
$Secret = $params['Secret']
$Stage = $params['Stage']
$Report = $params['Report']
$RestrictingSid = $params['RestrictingSid']
$Git = $params['Git']
$Probe = $params['Probe']
$Nonce = $params['Nonce']
$EntryPath = $params['EntryPath']
$ProofPath = $params['ProofPath']
$HostWritePath = $params['HostWritePath']
$AliasFile = $params['AliasFile']
foreach ($required in @('Secret','Stage','Report','RestrictingSid','Git','Probe','Nonce','EntryPath','ProofPath','HostWritePath','AliasFile')) {
    if ([string]::IsNullOrWhiteSpace($params[$required])) { throw "Runner params file is missing $required." }
}

function Start-RestrictedChild([IntPtr] $Token, [string] $Application, [string] $Arguments, [string] $WorkDir, [uint32] $WaitMs) {
    $info = [C044ProcessInformation]::new()
    $startup = [C044StartupInfo]::new()
    $startup.cb = [Runtime.InteropServices.Marshal]::SizeOf([type][C044StartupInfo])
    $line = [Text.StringBuilder]::new($Arguments)
    $created = [C044Native]::CreateProcessAsUserW($Token, $Application, $line, [IntPtr]::Zero, [IntPtr]::Zero, $false, 0x00000004, [IntPtr]::Zero, $WorkDir, [ref]$startup, [ref]$info)
    if (-not $created) { return @{ created = $false; error = [Runtime.InteropServices.Marshal]::GetLastWin32Error() } }
    try {
        $inJob = $false
        if (-not [C044Native]::IsProcessInJob($info.hProcess, [IntPtr]::Zero, [ref]$inJob) -or -not $inJob) {
            return @{ created = $true; job_verified = $false }
        }
        $suspend = [C044Native]::ResumeThread($info.hThread)
        $wait = [C044Native]::WaitForSingleObject($info.hProcess, $WaitMs)
        $exit = 0
        if ($wait -eq 0) { [void][C044Native]::GetExitCodeProcess($info.hProcess, [ref]$exit) }
        return @{ created = $true; job_verified = $true; suspend_count = $suspend; wait = $wait; exit_code = $exit; pid = $info.dwProcessId }
    } finally {
        if ($info.hThread -ne [IntPtr]::Zero) { [void][C044Native]::CloseHandle($info.hThread) }
        if ($info.hProcess -ne [IntPtr]::Zero) { [void][C044Native]::CloseHandle($info.hProcess) }
    }
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$normalRead = 'failed'
try { $normalRead = if ([IO.File]::ReadAllText($Secret) -eq 'synthetic-c044-broad-readable-secret') { 'allowed' } else { 'wrong_content' } } catch { $normalRead = 'failed:' + $_.Exception.GetType().Name }
$sidPointer = [IntPtr]::Zero
$sidAttributes = [IntPtr]::Zero
$token = [IntPtr]::Zero
$child = [C044ProcessInformation]::new()
$created = $false
$errorCode = 0
$innerSuspendCount = 0
$innerJobVerified = $false
$innerResumeSucceeded = $false
$entrySeen = $false
$childGone = $false
$childExit = -1
$childExitHex = ''
$proof = $null
$shellResult = 'not_run'
$shellError = ''
$gitResult = 'not_run'
$gitError = ''
try {
    if (-not [C044Native]::ConvertStringSidToSidW($RestrictingSid, [ref]$sidPointer)) { throw "ConvertStringSidToSidW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    $sidAttributes = [Runtime.InteropServices.Marshal]::AllocHGlobal([IntPtr]::Size + 8)
    [Runtime.InteropServices.Marshal]::WriteIntPtr($sidAttributes, $sidPointer)
    [Runtime.InteropServices.Marshal]::WriteInt32($sidAttributes, [IntPtr]::Size, 0)
    $currentToken = $identity.Token
    if (-not [C044Native]::CreateRestrictedToken($currentToken, 0, 0, [IntPtr]::Zero, 0, [IntPtr]::Zero, 1, $sidAttributes, [ref]$token)) {
        throw "CreateRestrictedToken failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    $aliasArgs = ''
    $aliasPaths = @()
    if (-not [string]::IsNullOrWhiteSpace($AliasFile) -and (Test-Path -LiteralPath $AliasFile)) {
        $aliasPaths = @(Get-Content -LiteralPath $AliasFile -Encoding ascii) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    }
    foreach ($alias in $aliasPaths) { $aliasArgs += ' "' + $alias + '"' }
    $probeArgs = '"' + $Probe + '" "' + $EntryPath + '" "' + $ProofPath + '" "' + $Nonce + '" "' + $Secret + '" "' + $HostWritePath + '"' + $aliasArgs
    $startup = [C044StartupInfo]::new(); $startup.cb = [Runtime.InteropServices.Marshal]::SizeOf([type][C044StartupInfo])
    $commandLine = [Text.StringBuilder]::new($probeArgs)
    $created = [C044Native]::CreateProcessAsUserW($token, $Probe, $commandLine, [IntPtr]::Zero, [IntPtr]::Zero, $false, 0x00000004, [IntPtr]::Zero, $Stage, [ref]$startup, [ref]$child)
    if (-not $created) { $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error(); throw "CreateProcessAsUserW failed: $errorCode" }
    $inJob = $false
    if (-not [C044Native]::IsProcessInJob($child.hProcess, [IntPtr]::Zero, [ref]$inJob) -or -not $inJob) { throw 'Restricted native child did not inherit Job ownership.' }
    $innerJobVerified = $true
    $innerSuspendCount = [C044Native]::ResumeThread($child.hThread)
    if ($innerSuspendCount -ne 1) { throw "Native child suspend count was unexpected: $innerSuspendCount" }
    $innerResumeSucceeded = $true
    # Bounded entry poll that also watches for early child death: a
    # loader/privilege failure kills the child in milliseconds with a
    # Win32 exit code, which is stronger evidence than a bare timeout.
    # Total bound stays 30 s (60 x 500 ms).
    for ($i = 0; $i -lt 60; $i++) {
        if (Test-Path -LiteralPath $EntryPath) { $entrySeen = $true; break }
        if ([C044Native]::WaitForSingleObject($child.hProcess, 500) -eq 0) { $childGone = $true; break }
    }
    $exitCode = [uint32]0
    [void][C044Native]::GetExitCodeProcess($child.hProcess, [ref]$exitCode)
    $childExit = [int]$exitCode
    $childExitHex = ('0x{0:X8}' -f $exitCode)
    if ($entrySeen) {
        $wait = [C044Native]::WaitForSingleObject($child.hProcess, 30000)
        if ($wait -ne 0) { throw "Restricted native child did not exit after entry (wait=$wait)." }
        [void][C044Native]::GetExitCodeProcess($child.hProcess, [ref]$exitCode)
        $childExit = [int]$exitCode
        $childExitHex = ('0x{0:X8}' -f $exitCode)
    } elseif ($childGone) {
        throw "Restricted native child exited before entry (exit=$childExitHex)."
    } else {
        throw 'Restricted native child timed out waiting for entry marker (30 s).'
    }
    $entry = [IO.File]::ReadAllText($EntryPath, [Text.Encoding]::Unicode) | ConvertFrom-Json
    $proof = [IO.File]::ReadAllText($ProofPath, [Text.Encoding]::Unicode) | ConvertFrom-Json
    if ($entry.nonce -ne $Nonce -or $proof.nonce -ne $Nonce) { throw 'Probe evidence nonce mismatch.' }
    if ($childExit -eq 0 -and $proof.broad_read_error -ne 0 -and $proof.host_write_error -ne 0) {
        $shellMarker = Join-Path $Stage 'shell-ok.txt'
        $shell = Start-RestrictedChild $token ($env:SystemRoot + '\System32\cmd.exe') ('"' + $env:SystemRoot + '\System32\cmd.exe" /d /c echo c044-shell-ok') $Stage 15000
        if ($shell.created -and $shell.job_verified -and $shell.wait -eq 0 -and $shell.exit_code -eq 0) { $shellResult = 'allowed' } else { $shellResult = 'failed'; $shellError = ($shell | ConvertTo-Json -Compress) }
        $repo = Join-Path $Stage 'git-repo'
        $gitInit = Start-RestrictedChild $token $Git ('"' + $Git + '" init --quiet "' + $repo + '"') $Stage 20000
        if ($gitInit.created -and $gitInit.job_verified -and $gitInit.wait -eq 0 -and $gitInit.exit_code -eq 0) {
            $gitStatus = Start-RestrictedChild $token $Git ('"' + $Git + '" -C "' + $repo + '" status --porcelain') $Stage 20000
            if ($gitStatus.created -and $gitStatus.job_verified -and $gitStatus.wait -eq 0 -and $gitStatus.exit_code -eq 0) { $gitResult = 'allowed' } else { $gitResult = 'failed'; $gitError = ($gitStatus | ConvertTo-Json -Compress) }
        } else { $gitResult = 'failed'; $gitError = ($gitInit | ConvertTo-Json -Compress) }
    }
    $broadDenied = $proof.broad_read_error -ne 0
    $hostDenied = $proof.host_write_error -ne 0
    $aliasDenied = $true
    foreach ($code in @($proof.alias_errors)) { if ([int]$code -eq 0) { $aliasDenied = $false } }
    $stageOk = $entrySeen -and ($childExit -eq 0)
    if ($normalRead -ne 'allowed') { $outcome = 'control-failed' }
    elseif (-not $entrySeen) { $outcome = 'blocked-before-entry' }
    elseif (-not $broadDenied -or -not $hostDenied -or -not $aliasDenied) { $outcome = 'negative' }
    elseif ($shellResult -eq 'allowed' -and $gitResult -eq 'allowed') { $outcome = 'strict-positive-candidate' }
    else { $outcome = 'partial-positive' }
    [ordered]@{
        contract = 'codegg.two_hop_c044.v1'; phase = 'runner'; mode = 'restricted-child'
        normal_account_same_file_read = $normalRead; runner_sid = $identity.User.Value
        restricting_sid = $RestrictingSid; outcome = $outcome
        child_reached_entry = $entrySeen; child_pid = [int]$entry.pid; child_exit_code = $childExit
        child_exit_code_hex = $childExitHex; child_died_before_entry = $false
        child_user_sid = [string]$proof.user; restricting_groups_observed = [int]$proof.restricting_groups
        broad_read = $(if ($broadDenied) { 'denied' } else { 'allowed' }); broad_read_error = [int]$proof.broad_read_error
        host_write = $(if ($hostDenied) { 'denied' } else { 'allowed' }); host_write_error = [int]$proof.host_write_error
        alias_count = [int]$proof.alias_count; alias_errors = @($proof.alias_errors); alias_paths = $aliasPaths
        stage_write = $(if ($stageOk) { 'allowed' } else { 'failed' })
        native_shell = $shellResult; shell_error = $shellError; git = $gitResult; git_error = $gitError
        inner_child_created = $created; inner_child_suspend_count = $innerSuspendCount
        inner_child_owned_job_verified_before_resume = $innerJobVerified
        inner_child_resume_succeeded = $innerResumeSucceeded
        network = 'not_tested'; production_admission = 'not_authorized'
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Report -Encoding utf8
} catch {
    [ordered]@{
        contract = 'codegg.two_hop_c044.v1'; phase = 'runner'
        normal_account_same_file_read = $normalRead; runner_sid = $identity.User.Value
        restricting_sid = $RestrictingSid; outcome = 'blocked-before-entry'
        launch_error = $errorCode; launch_failure = $_.Exception.Message; inner_child_created = $created
        inner_child_suspend_count = $innerSuspendCount
        inner_child_owned_job_verified_before_resume = $innerJobVerified
        inner_child_resume_succeeded = $innerResumeSucceeded
        child_reached_entry = $entrySeen; child_exit_code = $childExit
        child_exit_code_hex = $childExitHex; child_died_before_entry = $childGone
        native_shell = $shellResult; shell_error = $shellError; git = $gitResult; git_error = $gitError
        network = 'not_tested'; production_admission = 'not_authorized'
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Report -Encoding utf8
} finally {
    if ($child.hThread -ne [IntPtr]::Zero) { [void][C044Native]::CloseHandle($child.hThread) }
    if ($child.hProcess -ne [IntPtr]::Zero) { [void][C044Native]::CloseHandle($child.hProcess) }
    if ($token -ne [IntPtr]::Zero) { [void][C044Native]::CloseHandle($token) }
    if ($sidAttributes -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::FreeHGlobal($sidAttributes) }
    if ($sidPointer -ne [IntPtr]::Zero) { [void][C044Native]::LocalFree($sidPointer) }
}
'@
    $bootstrap = "try { Add-Type -TypeDefinition @'`n$nativeSource`n'@ } catch { [ordered]@{ contract = 'codegg.two_hop_c044.v1'; phase = 'native-bootstrap'; outcome = 'blocked-before-entry'; error = `$_.Exception.Message } | ConvertTo-Json -Compress | Set-Content -LiteralPath `$Report -Encoding utf8; exit 90 }`n"
    $firstLineEnd = $script.IndexOf("`n")
    $script = $script.Substring(0, $firstLineEnd + 1) + $bootstrap + $script.Substring($firstLineEnd + 1)
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
    # Runner inputs travel via a stage params file: keeps the first-hop
    # LogonW command line to three short args.
    $paramsFile = Join-Path $stage 'runner-params.txt'
    @(
        "Secret=$secret",
        "Stage=$stage",
        "Report=$reportPath",
        "RestrictingSid=$limitingSid",
        "Git=$git",
        "Probe=$probeStaged",
        "Nonce=$nonce",
        "EntryPath=$entryPath",
        "ProofPath=$proofPath",
        "HostWritePath=$hostWritePath",
        "AliasFile=$aliasFile"
    ) | Set-Content -LiteralPath $paramsFile -Encoding ascii
    $passwordBytes = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($securePassword)
    try { $plainPassword = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($passwordBytes) }
    finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($passwordBytes) }
    $args = @($runnerScript, 'runner', $paramsFile) | ForEach-Object { '"' + $_.Replace('"','\"') + '"' }
    $line = [Text.StringBuilder]::new('"' + $pwsh + '" -NoLogo -NoProfile -NonInteractive -File ' + ($args -join ' '))
    $startup = [C044StartupInfo]::new(); $startup.cb = [Runtime.InteropServices.Marshal]::SizeOf([type][C044StartupInfo])
    $processInfo = [C044ProcessInformation]::new()
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
    $acctCheck = Get-LocalUser -Name $account
    Write-Output "C044_ACCOUNT_DIAGNOSTIC enabled=$($acctCheck.Enabled) principal_source=$($acctCheck.PrincipalSource) cmdline_len=$($line.Length) stage_exists=$(Test-Path -LiteralPath $stage)"
    $logonToken = [IntPtr]::Zero
    $logonOk = [C044Native]::LogonUserW($account, $null, $plainPassword, 2, 0, [ref]$logonToken)
    $logonError = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    if ($logonToken -ne [IntPtr]::Zero) { [void][C044Native]::CloseHandle($logonToken) }
    Write-Output "C044_LOGON_DIAGNOSTIC logon_ok=$logonOk logon_error=$logonError"
    $created = [C044Native]::CreateProcessWithLogonW($account, $null, $plainPassword, 1, $pwsh, $line, 0x00000404, $environmentBlock, $stage, [ref]$startup, [ref]$processInfo)
    [Runtime.InteropServices.Marshal]::FreeHGlobal($environmentBlock)
    $plainPassword = $null
    if (-not $created) { throw "CreateProcessWithLogonW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    $job = [C044Native]::CreateJobObjectW([IntPtr]::Zero, $null)
    if ($job -eq [IntPtr]::Zero) { throw "CreateJobObjectW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    $limits = [C044JobExtendedLimitInformation]::new(); $basic = $limits.BasicLimitInformation; $basic.LimitFlags = 0x00002000; $limits.BasicLimitInformation = $basic
    $size = [uint32][Runtime.InteropServices.Marshal]::SizeOf([type][C044JobExtendedLimitInformation])
    if (-not [C044Native]::SetInformationJobObject($job, 9, [ref]$limits, $size)) { throw "SetInformationJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    if (-not [C044Native]::AssignProcessToJobObject($job, $processInfo.hProcess)) { throw "AssignProcessToJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    $runnerInJob = $false
    if (-not [C044Native]::IsProcessInJob($processInfo.hProcess, $job, [ref]$runnerInJob) -or -not $runnerInJob) { throw 'First-hop runner was not verified in the owned Job before resume.' }
    $outerSuspendCount = [C044Native]::ResumeThread($processInfo.hThread)
    if ($outerSuspendCount -ne 1) { throw "First-hop runner suspend count was unexpected: $outerSuspendCount" }
    $wait = [C044Native]::WaitForSingleObject($processInfo.hProcess, 240000)
    if ($wait -ne 0) { [void][C044Native]::TerminateJobObject($job, 1); throw "First-hop runner did not exit (wait=$wait)." }
    if (-not (Test-Path -LiteralPath $reportPath)) { throw 'Two-hop runner produced no result report.' }
    $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    $report | Add-Member -NotePropertyName contract -NotePropertyValue 'codegg.two_hop_c044.v1' -Force
    $report | Add-Member -NotePropertyName child_reached_entry -NotePropertyValue ([bool]$report.child_reached_entry) -Force
    $report | Add-Member -NotePropertyName host_edition -NotePropertyValue (Get-CimInstance Win32_OperatingSystem).Caption
    $report | Add-Member -NotePropertyName host_build -NotePropertyValue (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuild
    $report | Add-Member -NotePropertyName host_ubr -NotePropertyValue (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').UBR
    $report | Add-Member -NotePropertyName host_arch -NotePropertyValue ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString())
    $report | Add-Member -NotePropertyName runner_executable -NotePropertyValue $pwsh
    $report | Add-Member -NotePropertyName runner_executable_sha256 -NotePropertyValue (Get-FileHash -LiteralPath $pwsh -Algorithm SHA256).Hash
    $report | Add-Member -NotePropertyName immutable_runner_script_sha256 -NotePropertyValue $scriptHash
    $report | Add-Member -NotePropertyName native_probe_sha256 -NotePropertyValue $stagedHash
    $report | Add-Member -NotePropertyName native_probe_compiler -NotePropertyValue $env:C044_PROBE_COMPILER
    $report | Add-Member -NotePropertyName first_hop_suspend_count -NotePropertyValue $outerSuspendCount
    $report | Add-Member -NotePropertyName first_hop_owned_job_verified_before_resume -NotePropertyValue $runnerInJob
    $report | Add-Member -NotePropertyName host_fixture_dacl_unchanged -NotePropertyValue ($hostDaclBefore -eq (Get-Acl -LiteralPath $hostRoot).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::All))
    $report | Add-Member -NotePropertyName secret_dacl_unchanged -NotePropertyValue ($secretDaclBefore -eq (Get-Acl -LiteralPath $secret).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::All))
    $report | Add-Member -NotePropertyName secret_sha256_unchanged -NotePropertyValue ($secretHashBefore -eq (Get-FileHash -LiteralPath $secret -Algorithm SHA256).Hash)
    $report | Add-Member -NotePropertyName stage_dacl_unchanged -NotePropertyValue ($stageDaclBefore -eq (Get-Acl -LiteralPath $stage).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::All))
    $report | Add-Member -NotePropertyName original_fixture_metadata_changed -NotePropertyValue $false
    $report | Add-Member -NotePropertyName account_profile_teardown -NotePropertyValue 'deferred to final cleanup; see C044_CLEANUP'
    $report | Add-Member -NotePropertyName git_sha -NotePropertyValue ([string]$env:GITHUB_SHA)
    $report | Add-Member -NotePropertyName workflow_run_id -NotePropertyValue ([string]$env:GITHUB_RUN_ID)
    $report | Add-Member -NotePropertyName qualification -NotePropertyValue 'Hosted disposable-runner diagnostic only; not an independent security admission.'
    $json = $report | ConvertTo-Json -Depth 8 -Compress
    "C044_REPORT=$json"
    if ($env:GITHUB_STEP_SUMMARY) { $json | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY }
} finally {
    if ($job -ne [IntPtr]::Zero) { [void][C044Native]::CloseHandle($job) }
    if ($processInfo -and $processInfo.hThread -ne [IntPtr]::Zero) { [void][C044Native]::CloseHandle($processInfo.hThread) }
    if ($processInfo -and $processInfo.hProcess -ne [IntPtr]::Zero) { [void][C044Native]::CloseHandle($processInfo.hProcess) }
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
    Write-Output "C044_CLEANUP principal_removed=$(-not (Get-LocalUser -Name $account -ErrorAction SilentlyContinue)) profile_removed=$profileRemoved fixtures_removed=$(-not (Test-Path -LiteralPath $root))"
}
