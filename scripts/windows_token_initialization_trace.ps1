$ErrorActionPreference = 'Stop'

$runId = [Guid]::NewGuid().ToString('N')
$account = "c028_$($runId.Substring(0, 8))"
$root = Join-Path $env:RUNNER_TEMP "codegg-c028-$runId"
$accountCreated = $false
$baseToken = [IntPtr]::Zero
$passwordSecure = $null
$password = $null

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
[StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
public struct C028StartupInfo {
 public int cb; public string lpReserved, lpDesktop, lpTitle;
 public int dwX,dwY,dwXSize,dwYSize,dwXCountChars,dwYCountChars,dwFillAttribute,dwFlags;
 public short wShowWindow,cbReserved2; public IntPtr lpReserved2,hStdInput,hStdOutput,hStdError;
}
[StructLayout(LayoutKind.Sequential)]
public struct C028ProcessInformation { public IntPtr hProcess,hThread; public uint dwProcessId,dwThreadId; }
[StructLayout(LayoutKind.Sequential)]
public struct C028JobBasicLimitInformation {
 public long PerProcessUserTimeLimit,PerJobUserTimeLimit; public uint LimitFlags;
 public UIntPtr MinimumWorkingSetSize,MaximumWorkingSetSize; public uint ActiveProcessLimit;
 public UIntPtr Affinity; public uint PriorityClass,SchedulingClass;
}
[StructLayout(LayoutKind.Sequential)]
public struct C028IoCounters { public ulong ReadOperationCount,WriteOperationCount,OtherOperationCount,ReadTransferCount,WriteTransferCount,OtherTransferCount; }
[StructLayout(LayoutKind.Sequential)]
public struct C028JobExtendedLimitInformation {
 public C028JobBasicLimitInformation BasicLimitInformation; public C028IoCounters IoInfo;
 public UIntPtr ProcessMemoryLimit,JobMemoryLimit,PeakProcessMemoryUsed,PeakJobMemoryUsed;
}
public static class C028Native {
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,ExactSpelling=true,SetLastError=true)]
 public static extern bool LogonUserW(string user,string domain,string password,int type,int provider,out IntPtr token);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,ExactSpelling=true,SetLastError=true)]
 public static extern bool CreateProcessWithTokenW(IntPtr token,uint logonFlags,string app,StringBuilder command,uint flags,IntPtr environment,string cwd,ref C028StartupInfo startup,out C028ProcessInformation info);
 [DllImport("userenv.dll",ExactSpelling=true,SetLastError=true)] public static extern bool CreateEnvironmentBlock(out IntPtr env,IntPtr token,bool inherit);
 [DllImport("userenv.dll",ExactSpelling=true,SetLastError=true)] public static extern bool DestroyEnvironmentBlock(IntPtr env);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,ExactSpelling=true,SetLastError=true)] public static extern IntPtr CreateJobObjectW(IntPtr attrs,string name);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool SetInformationJobObject(IntPtr job,int cls,ref C028JobExtendedLimitInformation info,uint size);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool AssignProcessToJobObject(IntPtr job,IntPtr process);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern uint ResumeThread(IntPtr thread);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern uint WaitForSingleObject(IntPtr handle,uint ms);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool GetExitCodeProcess(IntPtr process,out uint code);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool TerminateJobObject(IntPtr job,uint code);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool CloseHandle(IntPtr handle);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool ProcessIdToSessionId(uint pid,out uint session);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool IsProcessInJob(IntPtr process,IntPtr job,out bool result);
 [DllImport("advapi32.dll",SetLastError=true)] public static extern bool OpenProcessToken(IntPtr process,uint access,out IntPtr token);
 [DllImport("advapi32.dll",SetLastError=true)] public static extern bool GetTokenInformation(IntPtr token,int cls,IntPtr buffer,uint len,out uint needed);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,ExactSpelling=true,SetLastError=true)] public static extern bool ConvertSidToStringSidW(IntPtr sid,out IntPtr text);
 [DllImport("kernel32.dll",ExactSpelling=true)] public static extern IntPtr LocalFree(IntPtr p);
}
'@

function Get-ChildTokenFacts([IntPtr] $process) {
    $token = [IntPtr]::Zero
    $userBuffer = [IntPtr]::Zero
    $userText = [IntPtr]::Zero
    try {
        if (-not [C028Native]::OpenProcessToken($process, 0x0008, [ref]$token)) { throw "OpenProcessToken: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
        [uint32]$needed = 0
        [void][C028Native]::GetTokenInformation($token, 1, [IntPtr]::Zero, 0, [ref]$needed)
        $userBuffer = [Runtime.InteropServices.Marshal]::AllocHGlobal([int]$needed)
        if (-not [C028Native]::GetTokenInformation($token, 1, $userBuffer, $needed, [ref]$needed)) { throw "GetTokenInformation(TokenUser): $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
        $sid = [Runtime.InteropServices.Marshal]::ReadIntPtr($userBuffer)
        if (-not [C028Native]::ConvertSidToStringSidW($sid, [ref]$userText)) { throw "ConvertSidToStringSid: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
        $sidText = [Runtime.InteropServices.Marshal]::PtrToStringUni($userText)
        $sessionBuffer = [Runtime.InteropServices.Marshal]::AllocHGlobal(4)
        try {
            if (-not [C028Native]::GetTokenInformation($token, 12, $sessionBuffer, 4, [ref]$needed)) { throw "GetTokenInformation(TokenSessionId): $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
            $tokenSessionId=[Runtime.InteropServices.Marshal]::ReadInt32($sessionBuffer)
            if (-not [C028Native]::GetTokenInformation($token, 8, $sessionBuffer, 4, [ref]$needed)) { throw "GetTokenInformation(TokenType): $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
            [ordered]@{ user_sid = $sidText; token_session_id = $tokenSessionId; token_type = [Runtime.InteropServices.Marshal]::ReadInt32($sessionBuffer) }
        } finally { [Runtime.InteropServices.Marshal]::FreeHGlobal($sessionBuffer) }
    } finally {
        if ($userText -ne [IntPtr]::Zero) { [void][C028Native]::LocalFree($userText) }
        if ($userBuffer -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::FreeHGlobal($userBuffer) }
        if ($token -ne [IntPtr]::Zero) { [void][C028Native]::CloseHandle($token) }
    }
}

function Grant-TraceWorkspace([string] $Path,[string] $AccountSid,[bool] $ChildWritable) {
    $acl=[Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true,$false)
    foreach ($sid in @($AccountSid,'S-1-5-18',[Security.Principal.WindowsIdentity]::GetCurrent().User.Value)) {
        $rights=if($sid -eq $AccountSid -and -not $ChildWritable){[Security.AccessControl.FileSystemRights]::ReadAndExecute}else{[Security.AccessControl.FileSystemRights]::FullControl}
        $rule=[Security.AccessControl.FileSystemAccessRule]::new(
            [Security.Principal.SecurityIdentifier]::new($sid),$rights,
            [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit,
            [Security.AccessControl.PropagationFlags]::None,[Security.AccessControl.AccessControlType]::Allow)
        [void]$acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
}

function Start-TraceChild([string] $Name,[string] $Executable,[uint32] $LogonFlags,[string] $AccountSid) {
    $cwd = Join-Path $root $Name
    New-Item -ItemType Directory -Path $cwd | Out-Null
    Grant-TraceWorkspace -Path $cwd -AccountSid $AccountSid -ChildWritable $true
    $startup = [C028StartupInfo]::new()
    $startup.cb = [Runtime.InteropServices.Marshal]::SizeOf([type][C028StartupInfo])
    $process = [C028ProcessInformation]::new()
    $environment = [IntPtr]::Zero
    $job = [IntPtr]::Zero
    $events = [Collections.Generic.List[object]]::new()
    $profileAtEntry = $null
    $profileAtEntry = $null
    $createdAt = [DateTimeOffset]::UtcNow
    if (-not [C028Native]::CreateEnvironmentBlock([ref]$environment,$baseToken,$false)) { throw "CreateEnvironmentBlock: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    try {
        $command = [Text.StringBuilder]::new('"' + $Executable + '"')
        $flags = [uint32]0x00000404 # CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT
        if (-not [C028Native]::CreateProcessWithTokenW($baseToken,$LogonFlags,$Executable,$command,$flags,$environment,$cwd,[ref]$startup,[ref]$process)) {
            return [ordered]@{ name=$Name; created=$false; phase='CreateProcessWithTokenW'; win32_error=[Runtime.InteropServices.Marshal]::GetLastWin32Error(); logon_flags=$LogonFlags }
        }
        $job = [C028Native]::CreateJobObjectW([IntPtr]::Zero,$null)
        if ($job -eq [IntPtr]::Zero) { throw "CreateJobObjectW: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
        $limits = [C028JobExtendedLimitInformation]::new()
        $basic = $limits.BasicLimitInformation; $basic.LimitFlags=0x00002000; $limits.BasicLimitInformation=$basic
        $size=[uint32][Runtime.InteropServices.Marshal]::SizeOf([type][C028JobExtendedLimitInformation])
        if (-not [C028Native]::SetInformationJobObject($job,9,[ref]$limits,$size) -or -not [C028Native]::AssignProcessToJobObject($job,$process.hProcess)) { throw "Job setup: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
        $childFacts=Get-ChildTokenFacts $process.hProcess
        if ($childFacts.user_sid -ne $AccountSid -or $childFacts.token_type -ne 1) { throw 'Suspended child token SID/type mismatch.' }
        $inJob=$false
        if (-not [C028Native]::IsProcessInJob($process.hProcess,$job,[ref]$inJob) -or -not $inJob) { throw 'Suspended child was not verified in its kill-on-close Job.' }
        [uint32]$childSession=0
        if (-not [C028Native]::ProcessIdToSessionId($process.dwProcessId,[ref]$childSession)) { throw "ProcessIdToSessionId: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
        $events.Add([ordered]@{ phase='created-suspended'; at=[DateTimeOffset]::UtcNow; process_session_id=$childSession; token=$childFacts })
        $resumePrevious=[C028Native]::ResumeThread($process.hThread)
        if ($resumePrevious -eq [uint32]::MaxValue) { throw "ResumeThread: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
        $events.Add([ordered]@{ phase='resumed'; at=[DateTimeOffset]::UtcNow; previous_suspend_count=$resumePrevious })
        $deadline=[DateTimeOffset]::UtcNow.AddSeconds(15)
        while ([DateTimeOffset]::UtcNow -lt $deadline -and (Wait-Path -Path (Join-Path $cwd 'complete.marker') -TimeoutMs 100) -eq $false) {
            if ((Test-Path -LiteralPath (Join-Path $cwd 'entry.marker')) -and -not ($events | Where-Object phase -eq 'first-instruction')) {
                $profileAtEntry = Test-Path "Registry::HKEY_USERS\$AccountSid"
                $events.Add([ordered]@{ phase='first-instruction'; at=[DateTimeOffset]::UtcNow; marker=(Get-Content (Join-Path $cwd 'entry.marker') -Raw).Trim(); profile_hku_present=$profileAtEntry })
            }
            if ([C028Native]::WaitForSingleObject($process.hProcess,0) -eq 0) { break }
        }
        $entryPath=Join-Path $cwd 'entry.marker'; $completePath=Join-Path $cwd 'complete.marker'
        $entry=if(Test-Path $entryPath){(Get-Content $entryPath -Raw).Trim()}else{$null}
        $complete=Test-Path $completePath
        $context=if(Test-Path (Join-Path $cwd 'context.txt')){Get-Content (Join-Path $cwd 'context.txt') -Encoding Unicode}else{@()}
        $wait=[C028Native]::WaitForSingleObject($process.hProcess,0)
        $timedOut=$wait -ne 0
        if ($timedOut) { [void][C028Native]::TerminateJobObject($job,1); [void][C028Native]::WaitForSingleObject($process.hProcess,5000) }
        $exitCode=$null
        if (-not $timedOut) { [uint32]$exitCode=0; [void][C028Native]::GetExitCodeProcess($process.hProcess,[ref]$exitCode) }
        [ordered]@{ name=$Name; created=$true; logon_flags=$LogonFlags; created_at=$createdAt; events=$events; entry_marker=$entry; completed_marker=$complete; child_context=$context; timed_out=$timedOut; exit_code=$exitCode; profile_hku_present_at_first_instruction=$profileAtEntry; child_process_session_id=$childSession }
    } finally {
        if ($job -ne [IntPtr]::Zero) { [void][C028Native]::CloseHandle($job) }
        if ($process.hThread -ne [IntPtr]::Zero) { [void][C028Native]::CloseHandle($process.hThread) }
        if ($process.hProcess -ne [IntPtr]::Zero) { [void][C028Native]::CloseHandle($process.hProcess) }
        if ($environment -ne [IntPtr]::Zero) { [void][C028Native]::DestroyEnvironmentBlock($environment) }
    }
}

function Wait-Path([string] $Path,[int] $TimeoutMs) {
    $until=[DateTimeOffset]::UtcNow.AddMilliseconds($TimeoutMs)
    while ([DateTimeOffset]::UtcNow -lt $until) { if (Test-Path -LiteralPath $Path) { return $true }; Start-Sleep -Milliseconds 10 }
    Test-Path -LiteralPath $Path
}

try {
    New-Item -ItemType Directory -Path $root | Out-Null
    $bytes=[byte[]]::new(36); [Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    $password=[Convert]::ToBase64String($bytes)+'aA!9'; [Array]::Clear($bytes,0,$bytes.Length)
    $passwordSecure=ConvertTo-SecureString $password -AsPlainText -Force
    New-LocalUser -Name $account -Password $passwordSecure -AccountNeverExpires -PasswordNeverExpires -Description 'Disposable CodeGG C028 trace principal' | Out-Null
    $accountCreated=$true
    $accountSid=(Get-LocalUser -Name $account).SID.Value
    Grant-TraceWorkspace -Path $root -AccountSid $accountSid -ChildWritable $false
    if (-not [C028Native]::LogonUserW($account,$env:COMPUTERNAME,$password,2,0,[ref]$baseToken)) { throw "LogonUserW: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    $vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $vsRoot=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
    if (-not $vsRoot) { throw 'Visual Studio C++ build tools unavailable.' }
    $vcvars=Join-Path $vsRoot 'VC\Auxiliary\Build\vcvars64.bat'
    $source=Join-Path $PSScriptRoot 'windows_token_initialization_marker.c'
    $crt=Join-Path $root 'c028-crt.exe'; $noCrt=Join-Path $root 'c028-no-crt.exe'
    $compileCrt='call "{0}" >nul && cl.exe /nologo /W4 /WX /MT /O2 /Fe:"{1}" "{2}" user32.lib' -f $vcvars,$crt,$source
    $compileNoCrt='call "{0}" >nul && cl.exe /nologo /W4 /WX /GS- /Zl /O2 /DC028_NO_CRT /Fe:"{1}" "{2}" /link /NODEFAULTLIB /ENTRY:C028NoCrtEntry /SUBSYSTEM:CONSOLE kernel32.lib user32.lib' -f $vcvars,$noCrt,$source
    Push-Location $root
    try { & $env:ComSpec /d /c $compileCrt; if ($LASTEXITCODE -ne 0) { throw "CRT fixture compile failed: $LASTEXITCODE" }; & $env:ComSpec /d /c $compileNoCrt; if ($LASTEXITCODE -ne 0) { throw "no-CRT fixture compile failed: $LASTEXITCODE" } }
    finally { Pop-Location }
    $privileges=(whoami /priv 2>&1 | Out-String).Trim()
    $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
    $session=0; [void][C028Native]::ProcessIdToSessionId([uint32]$PID,[ref]$session)
    $results=@(
        (Start-TraceChild 'crt-no-profile' $crt 0 $accountSid),
        (Start-TraceChild 'crt-with-profile' $crt 1 $accountSid),
        (Start-TraceChild 'no-crt-no-profile' $noCrt 0 $accountSid),
        (Start-TraceChild 'no-crt-with-profile' $noCrt 1 $accountSid)
    )
    foreach ($case in $results) {
        $case['failure_phase']=if(-not $case.created){'process-creation'}elseif($case.timed_out -and -not $case.entry_marker){'pre-user-entry'}elseif($case.timed_out -and $case.entry_marker -and -not $case.completed_marker){'after-entry-before-completion'}elseif($case.timed_out){'post-completion-before-exit'}elseif($case.exit_code -ne 0){'child-exit'}else{$null}
    }
    $allCompleted=@($results|Where-Object{$_.created -and $_.completed_marker -and -not $_.timed_out -and $_.exit_code -eq 0}).Count -eq 4
    $allPreEntryTimedOut=@($results|Where-Object{$_.created -and $_.timed_out -and -not $_.entry_marker}).Count -eq 4
    $os=Get-CimInstance Win32_OperatingSystem
    $report=[ordered]@{
        contract='codegg.windows_token_initialization_trace.v1'; source_sha=(git rev-parse HEAD).Trim()
        os=$os.Caption; version=$os.Version; build=$os.BuildNumber; ubr=(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').UBR
        runner_identity=$identity.Name; runner_privileges=$privileges; caller_session_id=$session; account_sid=$accountSid
        launch_api='CreateProcessWithTokenW'; executable_path='absolute'; command_line='quoted absolute path'; cwd='unique owned per-case directory'
        creation_flags='CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT'; handles_inherited=$false; environment='CreateEnvironmentBlock(target token)'; profile_flags=@{ absent=0; LOGON_WITH_PROFILE=1 }
        desktop='default inherited'; stdio='not redirected'; cases=$results
        disposition=if($allCompleted){'all-initialization-controls-completed'}elseif($allPreEntryTimedOut){'negative-pre-user-entry-timeout'}else{'phase-isolated-or-unresolved'}
        report_collection_complete=(@($results).Count -eq 4)
        production_admission=$false; restricted_read_claim=$false
    }
    $json=$report|ConvertTo-Json -Depth 10 -Compress
    "C028_REPORT=$json"
    if ($env:GITHUB_STEP_SUMMARY) { $json | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY }
    if (@($results).Count -ne 4) { throw 'C028 did not collect all four bounded case results.' }
} catch {
    "C028_FAILURE=$($_.Exception.ToString())"; throw
} finally {
    if ($baseToken -ne [IntPtr]::Zero) { [void][C028Native]::CloseHandle($baseToken) }
    if ($accountCreated) {
        Remove-LocalUser -Name $account -ErrorAction SilentlyContinue
        if (Get-LocalUser -Name $account -ErrorAction SilentlyContinue) { throw 'C028 cleanup failed to remove the disposable principal.' }
        'C028_CLEANUP principal_removed=true'
    }
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue }
    if (Test-Path -LiteralPath $root) { throw 'C028 cleanup failed to remove its unique fixture root.' }
    if ($passwordSecure) { $passwordSecure.Dispose() }
    $password=$null
}
