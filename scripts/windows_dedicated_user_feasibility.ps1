$ErrorActionPreference = 'Stop'

$runId = [Guid]::NewGuid().ToString('N')
$account = "c021_$($runId.Substring(0, 8))"
$accountCreated = $false
$hardlink = $null
$stage = Join-Path $env:RUNNER_TEMP "codegg-c021-stage-$runId"
$hostFixture = Join-Path $env:RUNNER_TEMP "codegg-c021-host-$runId"
$junction = Join-Path $env:RUNNER_TEMP "codegg-c021-junction-$runId"
$job = [IntPtr]::Zero
$processInfo = $null
$environmentPointer = [IntPtr]::Zero
$securePassword = $null
$secretHandleStream = $null
$operatorPipe = $null

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct C021StartupInfo {
    public int cb;
    public string lpReserved;
    public string lpDesktop;
    public string lpTitle;
    public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars;
    public int dwFillAttribute, dwFlags;
    public short wShowWindow, cbReserved2;
    public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
}

[StructLayout(LayoutKind.Sequential)]
public struct C021ProcessInformation {
    public IntPtr hProcess, hThread;
    public int dwProcessId, dwThreadId;
}

[StructLayout(LayoutKind.Sequential)]
public struct C021JobBasicLimitInformation {
    public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
    public uint LimitFlags;
    public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
    public uint ActiveProcessLimit;
    public UIntPtr Affinity;
    public uint PriorityClass, SchedulingClass;
}

[StructLayout(LayoutKind.Sequential)]
public struct C021IoCounters {
    public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
    public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
}

[StructLayout(LayoutKind.Sequential)]
public struct C021JobExtendedLimitInformation {
    public C021JobBasicLimitInformation BasicLimitInformation;
    public C021IoCounters IoInfo;
    public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
}

public static class C021Native {
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool CreateProcessWithLogonW(string user, string domain, string password,
        int logonFlags, string application, StringBuilder commandLine, int creationFlags,
        IntPtr environment, string currentDirectory, ref C021StartupInfo startup,
        out C021ProcessInformation processInformation);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetInformationJobObject(IntPtr job, int infoClass,
        ref C021JobExtendedLimitInformation info, uint length);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool TerminateJobObject(IntPtr job, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool TerminateProcess(IntPtr process, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr OpenProcess(uint access, bool inherit, int processId);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);
}
'@

function Get-DaclFingerprint([string] $Path) {
    $sddl = (Get-Acl -LiteralPath $Path).Sddl
    $bytes = [Text.Encoding]::UTF8.GetBytes($sddl)
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))
}

function Set-PrivateDirectoryAcl([string] $Path, [string[]] $Sids) {
    [void][IO.Directory]::CreateDirectory($Path)
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($sid in $Sids) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new(
            [Security.Principal.SecurityIdentifier]::new($sid),
            [Security.AccessControl.FileSystemRights]::FullControl,
            [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit,
            [Security.AccessControl.PropagationFlags]::None,
            [Security.AccessControl.AccessControlType]::Allow)
        [void]$acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
}

function Invoke-RunnerCommand([string] $File, [string] $Arguments) {
    $process = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($File, $Arguments))
    if (-not $process.WaitForExit(15000)) {
        $process.Kill()
        throw "Timed out running fixture setup command: $File"
    }
    if ($process.ExitCode -ne 0) {
        throw "Fixture setup command failed ($($process.ExitCode)): $File $Arguments"
    }
}

try {
    $adminIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $adminSid = $adminIdentity.User.Value
    $systemSid = 'S-1-5-18'
    $systemRoot = $env:SystemRoot
    $windowsPowerShell = Join-Path $systemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $git = (Get-Command git.exe -ErrorAction SilentlyContinue).Source
    $python = (Get-Command python.exe -ErrorAction SilentlyContinue).Source
    $cargo = (Get-Command cargo.exe -ErrorAction SilentlyContinue).Source

    $randomBytes = [byte[]]::new(36)
    [Security.Cryptography.RandomNumberGenerator]::Fill($randomBytes)
    $password = [Convert]::ToBase64String($randomBytes) + 'aA!9'
    [Array]::Clear($randomBytes, 0, $randomBytes.Length)
    $securePassword = ConvertTo-SecureString $password -AsPlainText -Force
    New-LocalUser -Name $account -Password $securePassword -AccountNeverExpires -PasswordNeverExpires `
        -Description 'Disposable CodeGG C021 feasibility principal' | Out-Null
    $accountCreated = $true
    $accountSid = (Get-LocalUser -Name $account).SID.Value
    $password = $null

    Set-PrivateDirectoryAcl -Path $stage -Sids @($accountSid, $adminSid, $systemSid)
    Set-PrivateDirectoryAcl -Path $hostFixture -Sids @($adminSid, $systemSid)
    $secret = Join-Path $hostFixture 'synthetic-operator-secret.txt'
    [IO.File]::WriteAllText($secret, 'synthetic-c021-secret')
    $caseAlias = Join-Path $hostFixture 'SYNTHETIC-OPERATOR-SECRET.TXT'
    $hardlink = Join-Path $env:RUNNER_TEMP "codegg-c021-hardlink-$runId.txt"
    $ads = "$secret`:c021"
    [IO.File]::WriteAllText($ads, 'synthetic-c021-ads')
    Invoke-RunnerCommand -File (Join-Path $systemRoot 'System32\cmd.exe') `
        -Arguments "/d /c mklink /J `"$junction`" `"$hostFixture`""
    Invoke-RunnerCommand -File (Join-Path $systemRoot 'System32\cmd.exe') `
        -Arguments "/d /c mklink /H `"$hardlink`" `"$secret`""

    $stageDaclBefore = Get-DaclFingerprint $stage
    $hostDaclBefore = Get-DaclFingerprint $hostFixture
    $secretHashBefore = (Get-FileHash -LiteralPath $secret -Algorithm SHA256).Hash
    if ([IO.File]::ReadAllText($secret) -ne 'synthetic-c021-secret') {
        throw 'Runner administrator cannot read the protected synthetic secret fixture.'
    }
    $secretHandleStream = [IO.File]::OpenRead($secret)
    if (-not [C021Native]::SetHandleInformation($secretHandleStream.SafeFileHandle.DangerousGetHandle(), 1, 1)) {
        throw "Could not mark the synthetic secret handle inheritable: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    $pipeName = "codegg-c021-$runId"
    $pipeSecurity = [IO.Pipes.PipeSecurity]::new()
    $pipeSecurity.SetAccessRuleProtection($true, $false)
    foreach ($sid in @($adminSid, $systemSid)) {
        $pipeRule = [IO.Pipes.PipeAccessRule]::new(
            [Security.Principal.SecurityIdentifier]::new($sid),
            [IO.Pipes.PipeAccessRights]::FullControl,
            [Security.AccessControl.AccessControlType]::Allow)
        [void]$pipeSecurity.AddAccessRule($pipeRule)
    }
    $operatorPipe = [IO.Pipes.NamedPipeServerStream]::new(
        $pipeName,
        [IO.Pipes.PipeDirection]::InOut,
        1,
        [IO.Pipes.PipeTransmissionMode]::Byte,
        [IO.Pipes.PipeOptions]::Asynchronous,
        0,
        0,
        $pipeSecurity)

    $resultPath = Join-Path $stage 'child-result.json'
    $childScriptPath = Join-Path $stage 'child-probe.ps1'
    $script = @"
`$observed = [ordered]@{}
`$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
`$principal = [Security.Principal.WindowsPrincipal]::new(`$identity)
`$observed.child_sid = `$identity.User.Value
`$observed.child_name = `$identity.Name
`$observed.child_is_administrator = `$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
`$observed.child_group_sids = @(`$identity.Groups | ForEach-Object { `$_.Value })
try { [IO.File]::WriteAllText([IO.Path]::Combine(`$env:C021_STAGE, 'stage-write.txt'), 'stage-ok'); `$observed.stage_write = 'allowed' } catch { `$observed.stage_write = 'failed:' + `$_.Exception.GetType().Name }
foreach (`$probe in @(
    @{ name='direct'; path=`$env:C021_SECRET },
    @{ name='junction'; path=`$env:C021_JUNCTION },
    @{ name='case_alias'; path=`$env:C021_CASE_ALIAS },
    @{ name='hardlink'; path=`$env:C021_HARDLINK },
    @{ name='ads'; path=`$env:C021_ADS }
)) { try { [void][IO.File]::ReadAllText(`$probe.path); `$observed["read_`$(`$probe.name)"] = 'allowed' } catch { `$observed["read_`$(`$probe.name)"] = 'denied' } }
try { [IO.File]::WriteAllText(`$env:C021_OUTSIDE_WRITE, 'synthetic-write'); `$observed.outside_create_write = 'allowed' } catch { `$observed.outside_create_write = 'denied' }
`$moveSource = [IO.Path]::Combine(`$env:C021_STAGE, 'move-source.txt')
[IO.File]::WriteAllText(`$moveSource, 'synthetic-move')
try { [IO.File]::Move(`$moveSource, `$env:C021_OUTSIDE_MOVE); `$observed.outside_move = 'allowed' } catch { `$observed.outside_move = 'denied' }
try { [IO.File]::Delete(`$env:C021_SECRET); `$observed.outside_delete = 'allowed' } catch { `$observed.outside_delete = 'denied' }
`$observed.git = 'not_available'
if (`$env:C021_GIT) { `$repo = [IO.Path]::Combine(`$env:C021_STAGE, 'git-repo'); & `$env:C021_GIT init --quiet `$repo; `$initExit = `$LASTEXITCODE; if (`$initExit -eq 0) { & `$env:C021_GIT -C `$repo status --porcelain; `$observed.git = if (`$LASTEXITCODE -eq 0) {'allowed'} else {'failed'} } else { `$observed.git = 'failed' } }
foreach (`$tool in @(@{name='python'; path=`$env:C021_PYTHON}, @{name='cargo'; path=`$env:C021_CARGO})) { if (`$tool.path) { & `$tool.path --version; `$observed[`$tool.name] = if (`$LASTEXITCODE -eq 0) {'allowed'} else {'failed'} } else { `$observed[`$tool.name] = 'not_available' } }
`$observed.descendant_pid = 0
try {
    `$pipeClient = [IO.Pipes.NamedPipeClientStream]::new('.', `$env:C021_PIPE, [IO.Pipes.PipeDirection]::InOut)
    `$pipeClient.Connect(1000)
    `$observed.operator_pipe = 'connected'
    `$pipeClient.Dispose()
} catch { `$observed.operator_pipe = 'denied' }
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class C021ChildHandleProbe {
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern uint GetFileType(IntPtr handle);
}
'@
try {
    `$candidateHandle = [IntPtr]::new([Int64]::Parse(`$env:C021_SECRET_HANDLE))
    if ([C021ChildHandleProbe]::GetFileType(`$candidateHandle) -eq 1) {
        `$safeHandle = [Microsoft.Win32.SafeHandles.SafeFileHandle]::new(`$candidateHandle, `$false)
        `$stream = [IO.FileStream]::new(`$safeHandle, [IO.FileAccess]::Read)
        `$reader = [IO.StreamReader]::new(`$stream)
        `$observed.inherited_secret_handle = if (`$reader.ReadToEnd() -eq 'synthetic-c021-secret') {'leaked'} else {'not_transferred'}
        `$reader.Dispose()
    } else { `$observed.inherited_secret_handle = 'not_transferred' }
} catch { `$observed.inherited_secret_handle = 'not_transferred' }
`$descendantScript = '[Threading.Thread]::Sleep(60000)'
`$descendant = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new(`$PSHOME + '\powershell.exe', '-NoProfile -NonInteractive -EncodedCommand ' + [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes(`$descendantScript))))
`$observed.descendant_pid = `$descendant.Id
[IO.File]::WriteAllText(`$env:C021_RESULT, (`$observed | ConvertTo-Json -Compress))
"@
    $environment = [ordered]@{
        C021_STAGE = $stage
        SystemDrive = $env:SystemDrive
        SystemRoot = $systemRoot
        WINDIR = $systemRoot
        USERPROFILE = Join-Path $env:SystemDrive "Users\$account"
        HOME = Join-Path $env:SystemDrive "Users\$account"
        LOCALAPPDATA = Join-Path $env:SystemDrive "Users\$account\AppData\Local"
        APPDATA = Join-Path $env:SystemDrive "Users\$account\AppData\Roaming"
        TEMP = $stage
        TMP = $stage
        PATH = "$systemRoot\System32;$systemRoot;$systemRoot\System32\Wbem;$systemRoot\System32\WindowsPowerShell\v1.0"
        C021_RESULT = $resultPath
        C021_SECRET_HANDLE = [string]$secretHandleStream.SafeFileHandle.DangerousGetHandle().ToInt64()
        C021_PIPE = $pipeName
        C021_GIT = $git
        C021_PYTHON = $python
        C021_CARGO = $cargo
        C021_SECRET = $secret
        C021_JUNCTION = Join-Path $junction 'synthetic-operator-secret.txt'
        C021_CASE_ALIAS = $caseAlias
        C021_HARDLINK = $hardlink
        C021_ADS = $ads
        C021_OUTSIDE_WRITE = Join-Path $hostFixture 'child-write.txt'
        C021_OUTSIDE_MOVE = Join-Path $hostFixture 'child-move.txt'
    }
    $childEnvironmentSetup = ($environment.GetEnumerator() |
        Where-Object { $_.Key.StartsWith('C021_', [StringComparison]::Ordinal) } |
        ForEach-Object {
            '$env:' + $_.Key + ' = ' + (ConvertTo-Json -InputObject ([string]$_.Value) -Compress)
        }) -join "`n"
    $script = $childEnvironmentSetup + "`n" + $script
    [IO.File]::WriteAllText($childScriptPath, $script, [Text.Encoding]::Unicode)
    $commandText = "`"$windowsPowerShell`" -NoLogo -NoProfile -NonInteractive -File `"$childScriptPath`""
    $commandLine = [Text.StringBuilder]::new(32768)
    [void]$commandLine.Append($commandText)
    $startup = [C021StartupInfo]::new()
    $startup.cb = [Runtime.InteropServices.Marshal]::SizeOf([type][C021StartupInfo])
    $processInfo = [C021ProcessInformation]::new()
    $createFlags = 0x00000004
    "C021_LAUNCH_DIAG=$([ordered]@{ startup_size = $startup.cb; application_exists = (Test-Path -LiteralPath $windowsPowerShell); current_directory_exists = (Test-Path -LiteralPath $stage); command_line_chars = $commandLine.Length; create_flags = $createFlags; environment_pointer_is_null = $true } | ConvertTo-Json -Compress)"

    $passwordBstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($securePassword)
    try {
        $plainPassword = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($passwordBstr)
        $created = [C021Native]::CreateProcessWithLogonW(
            $account, $null, $plainPassword, 0, $windowsPowerShell, $commandLine,
            $createFlags, [IntPtr]::Zero, $stage, [ref]$startup, [ref]$processInfo)
    } finally {
        if ($passwordBstr -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($passwordBstr) }
        $plainPassword = $null
    }
    if (-not $created) {
        $launchError = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw "CreateProcessWithLogonW failed: $launchError (0x$([Convert]::ToString($launchError, 16)))"
    }

    $job = [C021Native]::CreateJobObjectW([IntPtr]::Zero, $null)
    if ($job -eq [IntPtr]::Zero) { throw "CreateJobObjectW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    $limits = [C021JobExtendedLimitInformation]::new()
    $basicLimits = $limits.BasicLimitInformation
    $basicLimits.LimitFlags = 0x00002000
    $limits.BasicLimitInformation = $basicLimits
    $limitBytes = [uint32][Runtime.InteropServices.Marshal]::SizeOf([type][C021JobExtendedLimitInformation])
    if (-not [C021Native]::SetInformationJobObject($job, 9, [ref]$limits, $limitBytes)) {
        throw "SetInformationJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    if (-not [C021Native]::AssignProcessToJobObject($job, $processInfo.hProcess)) {
        throw "AssignProcessToJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    $inJob = $false
    if (-not [C021Native]::IsProcessInJob($processInfo.hProcess, $job, [ref]$inJob) -or -not $inJob) {
        throw 'The suspended child was not verified inside its kill-on-close Job.'
    }
    $launchJobVerified = $true
    if ([C021Native]::ResumeThread($processInfo.hThread) -eq [uint32]::MaxValue) {
        throw "ResumeThread failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
    $wait = [C021Native]::WaitForSingleObject($processInfo.hProcess, 120000)
    if ($wait -ne 0) {
        [void][C021Native]::TerminateJobObject($job, 1)
        throw "Dedicated-principal child did not exit in 120 seconds (wait=$wait)."
    }
    $childExit = [uint32]0
    [void][C021Native]::GetExitCodeProcess($processInfo.hProcess, [ref]$childExit)
    if ($environmentPointer -ne [IntPtr]::Zero) {
        [Runtime.InteropServices.Marshal]::FreeHGlobal($environmentPointer)
        $environmentPointer = [IntPtr]::Zero
    }
    [void][C021Native]::CloseHandle($job)
    $job = [IntPtr]::Zero
    if (-not (Test-Path -LiteralPath $resultPath)) { throw "Dedicated-principal child produced no result file; exit=$childExit" }
    $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
    $descendantGone = $false
    if ([int]$result.descendant_pid -gt 0) {
        $descendantHandle = [C021Native]::OpenProcess(0x00100000, $false, [int]$result.descendant_pid)
        if ($descendantHandle -eq [IntPtr]::Zero) {
            $descendantGone = $true
        } else {
            $descendantGone = [C021Native]::WaitForSingleObject($descendantHandle, 5000) -eq 0
            [void][C021Native]::CloseHandle($descendantHandle)
        }
    }
    $hostDaclAfter = Get-DaclFingerprint $hostFixture
    $stageDaclAfter = Get-DaclFingerprint $stage
    $secretHashAfter = (Get-FileHash -LiteralPath $secret -Algorithm SHA256).Hash
    $report = [ordered]@{
        contract = 'codegg.windows_dedicated_user_feasibility.v1'
        host_edition = (Get-CimInstance Win32_OperatingSystem).Caption
        host_version = (Get-CimInstance Win32_OperatingSystem).Version
        host_build = (Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuild
        runner_identity = $adminIdentity.Name
        runner_is_administrator = $true
        principal_name = $account
        principal_sid = $accountSid
        principal_created = $accountCreated
        suspended_launch_job_before_resume = $launchJobVerified
        inherited_handles = 'An inheritable handle to the synthetic host secret was supplied by the parent; child could not use it. CreateProcessWithLogonW exposes no handle-inheritance parameter.'
        child_exit_code = $childExit
        child = $result
        descendant_killed_when_job_closed = $descendantGone
        stage_dacl_before_sha256 = $stageDaclBefore
        stage_dacl_after_sha256 = $stageDaclAfter
        host_fixture_dacl_before_sha256 = $hostDaclBefore
        host_fixture_dacl_after_sha256 = $hostDaclAfter
        host_secret_sha256_before = $secretHashBefore
        host_secret_sha256_after = $secretHashAfter
        original_host_acl_changed = $false
        network_isolation = 'not tested and not claimed'
    }
    $json = $report | ConvertTo-Json -Depth 8 -Compress
    "C021_REPORT=$json"
    $json | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY
    if (-not $launchJobVerified -or $result.stage_write -ne 'allowed' -or
        $result.child_sid -ne $accountSid -or $result.child_is_administrator -or
        $result.git -ne 'allowed' -or
        -not $descendantGone -or $hostDaclBefore -ne $hostDaclAfter -or
        $secretHashBefore -ne $secretHashAfter) {
        throw 'C021 strict identity, stage, Job, cleanup, or host-fixture invariant failed.'
    }
    if ($result.inherited_secret_handle -ne 'not_transferred') {
        throw 'C021 leaked the marked inheritable synthetic host-secret handle.'
    }
    if ($result.operator_pipe -ne 'denied') { throw 'C021 child connected to the synthetic operator-only named pipe.' }
    foreach ($name in @('direct','junction','case_alias','hardlink','ads')) {
        $readResult = $result.PSObject.Properties["read_$name"].Value
        if ($readResult -ne 'denied') { throw "C021 host read probe was not denied: $name" }
    }
    foreach ($name in @('outside_create_write','outside_move','outside_delete')) {
        if ($result.$name -ne 'denied') { throw "C021 host mutation probe was not denied: $name" }
    }
} catch {
    Write-Output "C021_FAILURE=$($_.Exception.ToString())"
    throw
} finally {
    if ($job -ne [IntPtr]::Zero) {
        if ($processInfo -and $processInfo.hProcess -ne [IntPtr]::Zero -and
            [C021Native]::WaitForSingleObject($processInfo.hProcess, 0) -eq 0x00000102) {
            [void][C021Native]::TerminateJobObject($job, 1)
        }
        [void][C021Native]::CloseHandle($job)
    } elseif ($processInfo -and $processInfo.hProcess -ne [IntPtr]::Zero -and
        [C021Native]::WaitForSingleObject($processInfo.hProcess, 0) -eq 0x00000102) {
        [void][C021Native]::TerminateProcess($processInfo.hProcess, 1)
    }
    if ($processInfo -and $processInfo.hProcess -ne [IntPtr]::Zero) { [void][C021Native]::CloseHandle($processInfo.hProcess) }
    if ($processInfo -and $processInfo.hThread -ne [IntPtr]::Zero) { [void][C021Native]::CloseHandle($processInfo.hThread) }
    if ($environmentPointer -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::FreeHGlobal($environmentPointer) }
    if ($secretHandleStream) { $secretHandleStream.Dispose() }
    if ($operatorPipe) { $operatorPipe.Dispose() }
    if ($accountCreated) {
        Remove-LocalUser -Name $account -ErrorAction SilentlyContinue
        if (-not (Get-LocalUser -Name $account -ErrorAction SilentlyContinue)) { Write-Output 'C021_CLEANUP principal_removed=true' }
    }
    if (Test-Path -LiteralPath $junction) { Remove-Item -LiteralPath $junction -Force -ErrorAction SilentlyContinue }
    if ($hardlink -and (Test-Path -LiteralPath $hardlink)) { Remove-Item -LiteralPath $hardlink -Force -ErrorAction SilentlyContinue }
    if (Test-Path -LiteralPath $hostFixture) { Remove-Item -LiteralPath $hostFixture -Recurse -Force -ErrorAction SilentlyContinue }
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue }
    Write-Output "C021_CLEANUP host_fixture_removed=$(-not (Test-Path -LiteralPath $hostFixture)) stage_removed=$(-not (Test-Path -LiteralPath $stage))"
    if ($securePassword) { $securePassword.Dispose() }
}
