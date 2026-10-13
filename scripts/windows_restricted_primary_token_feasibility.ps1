$ErrorActionPreference = 'Stop'

$runId = [Guid]::NewGuid().ToString('N')
$account = "c027_$($runId.Substring(0, 8))"
$accountCreated = $false
$root = Join-Path $env:RUNNER_TEMP "codegg-c027-$runId"
$stage = Join-Path $root 'stage'
$broadRoot = Join-Path $root 'broad-readable-host-fixture'
$privateRoot = Join-Path $root 'private-host-fixture'
$baseToken = [IntPtr]::Zero
$restrictedToken = [IntPtr]::Zero
$passwordSecure = $null
$password = $null

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct C027StartupInfo {
    public int cb;
    public string lpReserved, lpDesktop, lpTitle;
    public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars;
    public int dwFillAttribute, dwFlags;
    public short wShowWindow, cbReserved2;
    public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
}

[StructLayout(LayoutKind.Sequential)]
public struct C027ProcessInformation {
    public IntPtr hProcess, hThread;
    public uint dwProcessId, dwThreadId;
}

[StructLayout(LayoutKind.Sequential)]
public struct C027JobBasicLimitInformation {
    public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
    public uint LimitFlags;
    public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
    public uint ActiveProcessLimit;
    public UIntPtr Affinity;
    public uint PriorityClass, SchedulingClass;
}

[StructLayout(LayoutKind.Sequential)]
public struct C027IoCounters {
    public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount;
    public ulong ReadTransferCount, WriteTransferCount, OtherTransferCount;
}

[StructLayout(LayoutKind.Sequential)]
public struct C027JobExtendedLimitInformation {
    public C027JobBasicLimitInformation BasicLimitInformation;
    public C027IoCounters IoInfo;
    public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
}

[StructLayout(LayoutKind.Sequential)]
public struct C027ByHandleFileInformation {
    public uint FileAttributes;
    public uint CreationTimeLow, CreationTimeHigh;
    public uint LastAccessTimeLow, LastAccessTimeHigh;
    public uint LastWriteTimeLow, LastWriteTimeHigh;
    public uint VolumeSerialNumber, FileSizeHigh, FileSizeLow, NumberOfLinks;
    public uint FileIndexHigh, FileIndexLow;
}

public static class C027Native {
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    public static extern bool CreateProcessAsUserW(IntPtr token, string application,
        StringBuilder commandLine, IntPtr processAttributes, IntPtr threadAttributes,
        bool inheritHandles, uint creationFlags, IntPtr environment, string currentDirectory,
        ref C027StartupInfo startup, out C027ProcessInformation processInformation);
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    public static extern bool CreateProcessWithTokenW(IntPtr token, uint logonFlags,
        string application, StringBuilder commandLine, uint creationFlags, IntPtr environment,
        string currentDirectory, ref C027StartupInfo startup,
        out C027ProcessInformation processInformation);
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    public static extern bool CreateProcessWithLogonW(string user, string domain, string password,
        int logonFlags, string application, StringBuilder commandLine, uint creationFlags,
        IntPtr environment, string currentDirectory, ref C027StartupInfo startup,
        out C027ProcessInformation processInformation);
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    public static extern bool LogonUserW(string user, string domain, string password,
        int logonType, int logonProvider, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError = true)]
    public static extern bool CreateRestrictedToken(IntPtr existingToken, uint flags,
        uint disableSidCount, IntPtr sidsToDisable, uint deletePrivilegeCount,
        IntPtr privilegesToDelete, uint restrictedSidCount, IntPtr sidsToRestrict,
        out IntPtr newToken);
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    public static extern bool ConvertStringSidToSidW(string stringSid, out IntPtr sid);
    [DllImport("userenv.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    public static extern bool CreateEnvironmentBlock(out IntPtr environment, IntPtr token, bool inherit);
    [DllImport("userenv.dll", ExactSpelling = true, SetLastError = true)]
    public static extern bool DestroyEnvironmentBlock(IntPtr environment);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    public static extern IntPtr CreateJobObjectW(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetInformationJobObject(IntPtr job, int infoClass,
        ref C027JobExtendedLimitInformation info, uint length);
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
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
    [DllImport("advapi32.dll", SetLastError = true)]
    public static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError = true)]
    public static extern bool GetTokenInformation(IntPtr token, int informationClass,
        IntPtr information, uint informationLength, out uint returnLength);
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    public static extern bool ConvertSidToStringSidW(IntPtr sid, out IntPtr stringSid);
    [DllImport("advapi32.dll")]
    public static extern bool EqualSid(IntPtr first, IntPtr second);
    [DllImport("kernel32.dll", ExactSpelling = true)]
    public static extern IntPtr LocalFree(IntPtr memory);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    public static extern IntPtr CreateFileW(string path, uint access, uint share,
        IntPtr security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GetFileInformationByHandle(IntPtr handle,
        out C027ByHandleFileInformation information);

    public static bool VerifyChildToken(IntPtr process, string expectedUserSid,
                                        string expectedRestrictedSid, bool requireRestricted) {
        IntPtr token = IntPtr.Zero;
        IntPtr userBuffer = IntPtr.Zero;
        IntPtr restrictedBuffer = IntPtr.Zero;
        IntPtr expectedSid = IntPtr.Zero;
        try {
            if (!OpenProcessToken(process, 0x0008, out token)) return false; // TOKEN_QUERY
            uint required = 0;
            GetTokenInformation(token, 8, IntPtr.Zero, 0, out required); // TokenType
            if (required == 0) return false;
            IntPtr typeBuffer = Marshal.AllocHGlobal((int)required);
            try {
                if (!GetTokenInformation(token, 8, typeBuffer, required, out required) ||
                    Marshal.ReadInt32(typeBuffer) != 1) return false; // TokenPrimary
            } finally { Marshal.FreeHGlobal(typeBuffer); }

            required = 0;
            GetTokenInformation(token, 1, IntPtr.Zero, 0, out required); // TokenUser
            if (required == 0) return false;
            userBuffer = Marshal.AllocHGlobal((int)required);
            if (!GetTokenInformation(token, 1, userBuffer, required, out required)) return false;
            IntPtr userSid = Marshal.ReadIntPtr(userBuffer);
            IntPtr userSidText = IntPtr.Zero;
            if (!ConvertSidToStringSidW(userSid, out userSidText)) return false;
            try {
                if (!String.Equals(Marshal.PtrToStringUni(userSidText), expectedUserSid,
                                   StringComparison.OrdinalIgnoreCase)) return false;
            } finally { LocalFree(userSidText); }
            if (!requireRestricted) return true;

            if (!ConvertStringSidToSidW(expectedRestrictedSid, out expectedSid)) return false;
            required = 0;
            GetTokenInformation(token, 11, IntPtr.Zero, 0, out required); // TokenRestrictedSids
            if (required == 0) return false;
            restrictedBuffer = Marshal.AllocHGlobal((int)required);
            if (!GetTokenInformation(token, 11, restrictedBuffer, required, out required)) return false;
            uint count = (uint)Marshal.ReadInt32(restrictedBuffer);
            int firstSidOffset = IntPtr.Size == 8 ? 8 : 4;
            int entrySize = IntPtr.Size == 8 ? 16 : 8;
            for (uint index = 0; index < count; ++index) {
                IntPtr entry = IntPtr.Add(restrictedBuffer, firstSidOffset + checked((int)index) * entrySize);
                IntPtr sid = Marshal.ReadIntPtr(entry);
                if (EqualSid(sid, expectedSid)) return true;
            }
            return false;
        } finally {
            if (expectedSid != IntPtr.Zero) LocalFree(expectedSid);
            if (restrictedBuffer != IntPtr.Zero) Marshal.FreeHGlobal(restrictedBuffer);
            if (userBuffer != IntPtr.Zero) Marshal.FreeHGlobal(userBuffer);
            if (token != IntPtr.Zero) CloseHandle(token);
        }
    }
}
'@

function Set-FixtureAcl([string] $Path, [string[]] $Sids, [bool] $BroadRead = $false) {
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
    if ($BroadRead) {
        foreach ($sid in @('S-1-5-32-545', 'S-1-5-11', 'S-1-1-0')) {
            $rule = [Security.AccessControl.FileSystemAccessRule]::new(
                [Security.Principal.SecurityIdentifier]::new($sid),
                [Security.AccessControl.FileSystemRights]::ReadAndExecute,
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
    $hash = [Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($sddl))
    [ordered]@{ owner = $acl.Owner; sddl_sha256 = [Convert]::ToHexString($hash); sddl = $sddl }
}

function Get-FileIdentity([string] $Path) {
    $handle = [C027Native]::CreateFileW($Path, 0x80000000, 7, [IntPtr]::Zero, 3, 0x80, [IntPtr]::Zero)
    if ($handle -eq [IntPtr](-1)) { throw "CreateFileW metadata failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    try {
        $information = [C027ByHandleFileInformation]::new()
        if (-not [C027Native]::GetFileInformationByHandle($handle, [ref]$information)) {
            throw "GetFileInformationByHandle failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        }
        '{0:X8}:{1:X8}{2:X8}' -f $information.VolumeSerialNumber, $information.FileIndexHigh, $information.FileIndexLow
    } finally { [void][C027Native]::CloseHandle($handle) }
}

function Quote-CommandArgument([string] $Value) {
    '"' + $Value.Replace('"', '\"') + '"'
}

function Start-ProbeChild([string] $Mode, [IntPtr] $Token, [string] $Helper,
        [string] $BroadFile, [string] $PrivateFile, [string] $StagePath,
        [string] $RestrictingSid, [string] $ResultPath, [string] $GitExe,
        [bool] $ExpectRestricted = $false) {
    $arguments = @($BroadFile, $PrivateFile, (Join-Path $StagePath "write-$Mode.tmp"),
        $RestrictingSid, 'synthetic-c027-broad-secret', $ResultPath, $GitExe)
    $command = (Quote-CommandArgument $Helper) + ' ' + (($arguments | ForEach-Object { Quote-CommandArgument $_ }) -join ' ')
    $line = [Text.StringBuilder]::new($command)
    $startup = [C027StartupInfo]::new()
    $startup.cb = [Runtime.InteropServices.Marshal]::SizeOf([type][C027StartupInfo])
    $process = [C027ProcessInformation]::new()
    $environment = [IntPtr]::Zero
    $job = [IntPtr]::Zero
    if ($Token -ne [IntPtr]::Zero) {
        if (-not [C027Native]::CreateEnvironmentBlock([ref]$environment, $Token, $false)) {
            $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            return [ordered]@{ mode = $Mode; launch_api = $Mode; created = $false; phase = 'environment'; win32_error = $errorCode }
        }
    }
    try {
        $flags = [uint32]0x00000404 # CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT
        if ($Mode -eq 'normal-logon') {
            $created = [C027Native]::CreateProcessWithLogonW($account, $null, $password, 0,
                $Helper, $line, $flags, $environment, $StagePath, [ref]$startup, [ref]$process)
        } elseif ($Mode -eq 'create-process-as-user') {
            $created = [C027Native]::CreateProcessAsUserW($Token, $Helper, $line,
                [IntPtr]::Zero, [IntPtr]::Zero, $false, $flags, $environment, $StagePath,
                [ref]$startup, [ref]$process)
        } else {
            $created = [C027Native]::CreateProcessWithTokenW($Token, 0, $Helper, $line,
                $flags, $environment, $StagePath, [ref]$startup, [ref]$process)
        }
        if (-not $created) {
            $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            return [ordered]@{ mode = $Mode; launch_api = $Mode; created = $false; phase = 'create-process'; win32_error = $errorCode }
        }

        $job = [C027Native]::CreateJobObjectW([IntPtr]::Zero, $null)
        if ($job -eq [IntPtr]::Zero) { throw "CreateJobObjectW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
        $limits = [C027JobExtendedLimitInformation]::new()
        $basic = $limits.BasicLimitInformation
        $basic.LimitFlags = 0x00002000 # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        $limits.BasicLimitInformation = $basic
        $size = [uint32][Runtime.InteropServices.Marshal]::SizeOf([type][C027JobExtendedLimitInformation])
        if (-not [C027Native]::SetInformationJobObject($job, 9, [ref]$limits, $size)) {
            throw "SetInformationJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        }
        if (-not [C027Native]::AssignProcessToJobObject($job, $process.hProcess)) {
            throw "AssignProcessToJobObject failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        }
        $inJob = $false
        if (-not [C027Native]::IsProcessInJob($process.hProcess, $job, [ref]$inJob) -or -not $inJob) {
            throw 'Suspended child was not verified in its kill-on-close Job.'
        }
        if (-not [C027Native]::VerifyChildToken($process.hProcess, $accountSid,
                $RestrictingSid, $ExpectRestricted)) {
            throw 'Suspended child token user/type/restricting-SID provenance failed verification.'
        }
        if ([C027Native]::ResumeThread($process.hThread) -eq [uint32]::MaxValue) {
            throw "ResumeThread failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
        }
        $wait = [C027Native]::WaitForSingleObject($process.hProcess, 15000)
        $exitCode = $null
        if ($wait -eq 0) {
            $exitCode = [uint32]0
            if (-not [C027Native]::GetExitCodeProcess($process.hProcess, [ref]$exitCode)) {
                throw "GetExitCodeProcess failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
            }
        } elseif ($wait -eq 258) {
            [void][C027Native]::TerminateJobObject($job, 1)
            [void][C027Native]::WaitForSingleObject($process.hProcess, 5000)
        } else { throw "WaitForSingleObject failed: $wait" }
    } catch {
        if ($job -ne [IntPtr]::Zero) { [void][C027Native]::TerminateJobObject($job, 1) }
        [void][C027Native]::TerminateProcess($process.hProcess, 1)
        [void][C027Native]::WaitForSingleObject($process.hProcess, 5000)
        throw
    } finally {
        if ($job -ne [IntPtr]::Zero) { [void][C027Native]::CloseHandle($job) }
        if ($process.hThread -ne [IntPtr]::Zero) { [void][C027Native]::CloseHandle($process.hThread) }
        if ($process.hProcess -ne [IntPtr]::Zero) { [void][C027Native]::CloseHandle($process.hProcess) }
        if ($environment -ne [IntPtr]::Zero) { [void][C027Native]::DestroyEnvironmentBlock($environment) }
    }
    $result = if (Test-Path -LiteralPath $ResultPath) {
        Get-Content -LiteralPath $ResultPath -Raw | ConvertFrom-Json
    } else { $null }
    [ordered]@{ mode = $Mode; launch_api = $Mode; created = $true; timed_out = ($wait -eq 258); exit_code = $exitCode; result = $result }
}

try {
    New-Item -ItemType Directory -Path $root, $stage, $broadRoot, $privateRoot | Out-Null
    $runnerSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $systemSid = 'S-1-5-18'
    $limitingSid = "S-1-5-21-2800000000-2800000001-2800000002-$([int](Get-Random -Minimum 1000 -Maximum 2000000000))"
    $passwordBytes = [byte[]]::new(36)
    [Security.Cryptography.RandomNumberGenerator]::Fill($passwordBytes)
    $password = [Convert]::ToBase64String($passwordBytes) + 'aA!9'
    [Array]::Clear($passwordBytes, 0, $passwordBytes.Length)
    $passwordSecure = ConvertTo-SecureString $password -AsPlainText -Force
    New-LocalUser -Name $account -Password $passwordSecure -AccountNeverExpires -PasswordNeverExpires `
        -Description 'Disposable CodeGG C027 feasibility principal' | Out-Null
    $accountCreated = $true
    $accountSid = (Get-LocalUser -Name $account).SID.Value
    Set-FixtureAcl -Path $root -Sids @($runnerSid, $systemSid, $accountSid, $limitingSid)
    Set-FixtureAcl -Path $stage -Sids @($runnerSid, $systemSid, $accountSid, $limitingSid)
    Set-FixtureAcl -Path $broadRoot -Sids @($runnerSid, $systemSid) -BroadRead $true
    Set-FixtureAcl -Path $privateRoot -Sids @($runnerSid, $systemSid)

    $broadFile = Join-Path $broadRoot 'synthetic-broad-readable-secret.txt'
    $privateFile = Join-Path $privateRoot 'synthetic-private-secret.txt'
    [IO.File]::WriteAllText($broadFile, 'synthetic-c027-broad-secret')
    [IO.File]::WriteAllText($privateFile, 'synthetic-c027-private-secret')
    $broadBefore = Get-SddlFingerprint $broadFile
    $privateBefore = Get-SddlFingerprint $privateFile
    $broadIdentityBefore = Get-FileIdentity $broadFile
    $privateIdentityBefore = Get-FileIdentity $privateFile
    $broadHashBefore = (Get-FileHash -LiteralPath $broadFile -Algorithm SHA256).Hash
    $privateHashBefore = (Get-FileHash -LiteralPath $privateFile -Algorithm SHA256).Hash

    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere)) { throw 'Visual Studio vswhere.exe is unavailable; native C027 fixture cannot be built.' }
    $vsRoot = (& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1)
    if (-not $vsRoot) { throw 'Visual Studio C++ build tools are unavailable; native C027 fixture cannot be built.' }
    $vcvars = Join-Path $vsRoot 'VC\Auxiliary\Build\vcvars64.bat'
    $helper = Join-Path $root 'windows_restricted_primary_token_probe.exe'
    $source = Join-Path $PSScriptRoot 'windows_restricted_primary_token_probe.c'
    $compileLine = 'call "{0}" >nul && cl.exe /nologo /W4 /WX /MT /DUNICODE /D_UNICODE /O2 /Fe:"{1}" "{2}"' -f $vcvars, $helper, $source
    Push-Location $root
    try { $compileOutput = & $env:ComSpec /d /s /c "`"$compileLine`" 2>&1" }
    finally { Pop-Location }
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $helper)) {
        throw "C027 native fixture compile failed ($LASTEXITCODE): $($compileOutput -join ' ')"
    }
    $gitExe = (Get-Command git.exe -ErrorAction Stop).Source

    if (-not [C027Native]::LogonUserW($account, $env:COMPUTERNAME, $password, 2, 0, [ref]$baseToken)) {
        $logonError = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        $report = [ordered]@{ contract = 'codegg.windows_restricted_primary_token_feasibility.v1'; disposition = 'blocked-token-acquisition'; source_sha = (git rev-parse HEAD).Trim(); token_acquisition_win32_error = $logonError; normal_account_baseline = $normal }
        $json = $report | ConvertTo-Json -Depth 8 -Compress
        "C027_REPORT=$json"
        if ($env:GITHUB_STEP_SUMMARY) { $json | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY }
        return
    }

    $normalPath = Join-Path $stage 'normal-logon.json'
    $normal = Start-ProbeChild -Mode normal-logon -Token $baseToken -Helper $helper `
        -BroadFile $broadFile -PrivateFile $privateFile -StagePath $stage `
        -RestrictingSid $limitingSid -ResultPath $normalPath -GitExe $gitExe
    if (-not $normal.created -or $normal.timed_out -or $normal.exit_code -ne 0 -or
        -not $normal.result -or $normal.result.token_user -ne $accountSid -or
        $normal.result.broad_read_win32_error -ne 0 -or
        $normal.result.private_read_win32_error -ne 5 -or
        $normal.result.stage_read_win32_error -ne 0 -or $normal.result.stage_write_win32_error -ne 0) {
        throw "Disposable account baseline did not prove broad-read/private-deny/stage-write: $($normal | ConvertTo-Json -Compress -Depth 5)"
    }

    $operatorPrivileges = (whoami /priv 2>&1 | Out-String).Trim()
    $asUserPath = Join-Path $stage 'as-user-control.json'
    $withTokenPath = Join-Path $stage 'with-token-control.json'
    $asUser = Start-ProbeChild -Mode create-process-as-user -Token $baseToken -Helper $helper `
        -BroadFile $broadFile -PrivateFile $privateFile -StagePath $stage `
        -RestrictingSid $limitingSid -ResultPath $asUserPath -GitExe $gitExe
    $withToken = Start-ProbeChild -Mode create-process-with-token -Token $baseToken -Helper $helper `
        -BroadFile $broadFile -PrivateFile $privateFile -StagePath $stage `
        -RestrictingSid $limitingSid -ResultPath $withTokenPath -GitExe $gitExe
    $positiveControl = @($asUser, $withToken) | Where-Object {
        $_.created -and -not $_.timed_out -and $_.exit_code -eq 0 -and $_.result -and
        $_.result.token_user -eq $accountSid -and $_.result.token_type -eq 'primary' -and
        $_.result.broad_read_win32_error -eq 0 -and $_.result.private_read_win32_error -eq 5 -and
        $_.result.stage_read_win32_error -eq 0 -and $_.result.stage_write_win32_error -eq 0
    } | Select-Object -First 1

    $restricted = $null
    if ($positiveControl) {
        $sidPointer = [IntPtr]::Zero
        $sidArray = [IntPtr]::Zero
        try {
            if (-not [C027Native]::ConvertStringSidToSidW($limitingSid, [ref]$sidPointer)) {
                throw "ConvertStringSidToSidW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
            }
            $sidArray = [Runtime.InteropServices.Marshal]::AllocHGlobal([Runtime.InteropServices.Marshal]::SizeOf([type][IntPtr]) + 8)
            [Runtime.InteropServices.Marshal]::WriteIntPtr($sidArray, $sidPointer)
            [Runtime.InteropServices.Marshal]::WriteInt32($sidArray, [Runtime.InteropServices.Marshal]::SizeOf([type][IntPtr]), 0)
            # DISABLE_MAX_PRIVILEGE only: deliberately omit WRITE_RESTRICTED.
            if (-not [C027Native]::CreateRestrictedToken($baseToken, 1, 0, [IntPtr]::Zero,
                    0, [IntPtr]::Zero, 1, $sidArray, [ref]$restrictedToken)) {
                throw "CreateRestrictedToken failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
            }
        } finally {
            if ($sidArray -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::FreeHGlobal($sidArray) }
            if ($sidPointer -ne [IntPtr]::Zero) { [void][C027Native]::LocalFree($sidPointer) }
        }
        $restrictedPath = Join-Path $stage 'restricted-primary.json'
        $restricted = Start-ProbeChild -Mode create-process-as-user -Token $restrictedToken -Helper $helper `
            -BroadFile $broadFile -PrivateFile $privateFile -StagePath $stage `
            -RestrictingSid $limitingSid -ResultPath $restrictedPath -GitExe $gitExe -ExpectRestricted $true
        if (-not $restricted.created -and $positiveControl.mode -eq 'create-process-with-token') {
            $retryPath = Join-Path $stage 'restricted-primary-with-token.json'
            $restricted = Start-ProbeChild -Mode create-process-with-token -Token $restrictedToken -Helper $helper `
                -BroadFile $broadFile -PrivateFile $privateFile -StagePath $stage `
                -RestrictingSid $limitingSid -ResultPath $retryPath -GitExe $gitExe -ExpectRestricted $true
        }
    }

    $broadAfter = Get-SddlFingerprint $broadFile
    $privateAfter = Get-SddlFingerprint $privateFile
    $broadIdentityAfter = Get-FileIdentity $broadFile
    $privateIdentityAfter = Get-FileIdentity $privateFile
    $broadHashAfter = (Get-FileHash -LiteralPath $broadFile -Algorithm SHA256).Hash
    $privateHashAfter = (Get-FileHash -LiteralPath $privateFile -Algorithm SHA256).Hash
    $strictPositive = $restricted -and $restricted.created -and -not $restricted.timed_out -and
        $restricted.exit_code -eq 0 -and $restricted.result -and
        $restricted.result.token_user -eq $accountSid -and $restricted.result.token_type -eq 'primary' -and
        $restricted.result.limiting_sid_seen_in_child -and
        $restricted.result.private_read_win32_error -eq 5 -and
        $restricted.result.broad_read_win32_error -eq 5 -and
        $restricted.result.stage_read_win32_error -eq 0 -and $restricted.result.stage_write_win32_error -eq 0 -and
        $restricted.result.shell_start_win32_error -eq 0 -and $restricted.result.shell_exit_code -eq 0 -and
        $restricted.result.git_start_win32_error -eq 0 -and $restricted.result.git_exit_code -eq 0
    $disposition = if ($strictPositive) { 'bounded-read-boundary-positive-tool-matrix-incomplete' }
        elseif (-not $positiveControl) { 'negative-no-unrestricted-token-control' }
        elseif (-not $restricted -or -not $restricted.created) { 'negative-restricted-primary-launch' }
        elseif ($restricted.result -and $restricted.result.broad_read_win32_error -eq 0) { 'negative-restricted-child-read-broad-file' }
        else { 'negative-restricted-child-contract-incomplete' }
    $report = [ordered]@{
        contract = 'codegg.windows_restricted_primary_token_feasibility.v1'
        disposition = $disposition
        source_sha = (git rev-parse HEAD).Trim()
        os = (Get-CimInstance Win32_OperatingSystem).Caption
        version = (Get-CimInstance Win32_OperatingSystem).Version
        build = (Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuild
        ubr = (Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').UBR
        runner_identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
        runner_privileges = $operatorPrivileges
        account_sid = $accountSid
        restricting_sid = $limitingSid
        token_access_requested = 'TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY'
        process_start = 'absolute application path; mutable UTF-16 command line; explicit target-token Unicode environment; explicit stage cwd; suspended then assigned to kill-on-close Job then resumed; handles not inherited'
        profile_hkcu = [ordered]@{ normal_logon = 'not loaded'; create_process_as_user = 'not loaded'; create_process_with_token = 'not loaded' }
        normal_account_baseline = $normal
        create_process_as_user_unrestricted_account_control = $asUser
        create_process_with_token_unrestricted_account_control = $withToken
        selected_unrestricted_control = if ($positiveControl) { $positiveControl.mode } else { 'none' }
        restricted_primary_token_launch = $restricted
        broad_file = [ordered]@{ file_identity_before = $broadIdentityBefore; file_identity_after = $broadIdentityAfter; sddl_before = $broadBefore; sddl_after = $broadAfter; sha256_unchanged = ($broadHashBefore -eq $broadHashAfter) }
        private_file = [ordered]@{ file_identity_before = $privateIdentityBefore; file_identity_after = $privateIdentityAfter; sddl_before = $privateBefore; sddl_after = $privateAfter; sha256_unchanged = ($privateHashBefore -eq $privateHashAfter) }
        host_acl_mutation = $false
        aliases_ipc_handles_toolchain_descendants = 'not run; native cmd and Git --version smoke only after same-file denial'
        production_admission = $false
        network_isolation = 'not tested and not claimed'
    }
    $json = $report | ConvertTo-Json -Depth 10 -Compress
    "C027_REPORT=$json"
    if ($env:GITHUB_STEP_SUMMARY) { $json | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY }
    if ($broadBefore.sddl_sha256 -ne $broadAfter.sddl_sha256 -or
        $privateBefore.sddl_sha256 -ne $privateAfter.sddl_sha256 -or
        $broadIdentityBefore -ne $broadIdentityAfter -or $privateIdentityBefore -ne $privateIdentityAfter -or
        -not $report.broad_file.sha256_unchanged -or -not $report.private_file.sha256_unchanged) {
        throw 'C027 changed a source fixture descriptor, file identity, or content hash.'
    }
    if ($restricted.result -and $restricted.result.broad_read_win32_error -eq 0) {
        throw 'C027 security-negative: restricted primary-token child read the broad-readable source fixture.'
    }
    Write-Output "C027_CLEANUP principal_pending=$account"
} catch {
    Write-Output "C027_FAILURE=$($_.Exception.ToString())"
    throw
} finally {
    if ($restrictedToken -ne [IntPtr]::Zero) { [void][C027Native]::CloseHandle($restrictedToken) }
    if ($baseToken -ne [IntPtr]::Zero) { [void][C027Native]::CloseHandle($baseToken) }
    if ($accountCreated) {
        Remove-LocalUser -Name $account -ErrorAction SilentlyContinue
        if (-not (Get-LocalUser -Name $account -ErrorAction SilentlyContinue)) { Write-Output 'C027_CLEANUP principal_removed=true' }
    }
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue }
    if ($passwordSecure) { $passwordSecure.Dispose() }
    $password = $null
}
