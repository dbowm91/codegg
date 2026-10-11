$ErrorActionPreference = 'Stop'

if (-not $env:RUNNER_TEMP) {
    throw 'RUNNER_TEMP is required; this proof must use an isolated temporary root.'
}

$source = @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class AppContainerProofNative {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct StartupInfo {
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
    public struct StartupInfoEx {
        public StartupInfo StartupInfo;
        public IntPtr lpAttributeList;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct SecurityCapabilities {
        public IntPtr AppContainerSid;
        public IntPtr Capabilities;
        public uint CapabilityCount;
        public uint Reserved;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct ProcessInformation {
        public IntPtr hProcess, hThread;
        public uint dwProcessId, dwThreadId;
    }

    [DllImport("userenv.dll", CharSet = CharSet.Unicode)]
    public static extern int CreateAppContainerProfile(string name, string displayName,
        string description, IntPtr capabilities, uint capabilityCount, out IntPtr appContainerSid);
    [DllImport("userenv.dll", CharSet = CharSet.Unicode)]
    public static extern int DeleteAppContainerProfile(string name);
    [DllImport("advapi32.dll")]
    public static extern IntPtr FreeSid(IntPtr sid);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool InitializeProcThreadAttributeList(IntPtr list, int count,
        int flags, ref IntPtr size);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags,
        IntPtr attribute, IntPtr value, IntPtr size, IntPtr previous, IntPtr returnedSize);
    [DllImport("kernel32.dll")]
    public static extern void DeleteProcThreadAttributeList(IntPtr list);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool CreateProcess(string applicationName, StringBuilder commandLine,
        IntPtr processAttributes, IntPtr threadAttributes, bool inheritHandles,
        uint creationFlags, IntPtr environment, string currentDirectory,
        ref StartupInfoEx startupInfo, out ProcessInformation processInformation);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool TerminateProcess(IntPtr process, uint exitCode);
    [DllImport("kernel32.dll")]
    public static extern bool CloseHandle(IntPtr handle);

    public static uint Run(string executable, string arguments, IntPtr appContainerSid) {
        IntPtr SecurityCapabilitiesAttribute = (IntPtr)0x00020009;
        const uint ExtendedStartupInfoPresent = 0x00080000;
        const uint CreateNoWindow = 0x08000000;
        IntPtr size = IntPtr.Zero;
        InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref size);
        if (size == IntPtr.Zero) throw new InvalidOperationException("Could not size attribute list.");
        IntPtr attributes = Marshal.AllocHGlobal(size);
        ProcessInformation process = new ProcessInformation();
        try {
            if (!InitializeProcThreadAttributeList(attributes, 1, 0, ref size))
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            SecurityCapabilities capabilities = new SecurityCapabilities {
                AppContainerSid = appContainerSid,
                Capabilities = IntPtr.Zero,
                CapabilityCount = 0,
                Reserved = 0
            };
            IntPtr capabilitiesPtr = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(SecurityCapabilities)));
            try {
                Marshal.StructureToPtr(capabilities, capabilitiesPtr, false);
                if (!UpdateProcThreadAttribute(attributes, 0, SecurityCapabilitiesAttribute,
                        capabilitiesPtr, (IntPtr)Marshal.SizeOf(typeof(SecurityCapabilities)),
                        IntPtr.Zero, IntPtr.Zero))
                    throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
                StartupInfoEx startup = new StartupInfoEx();
                startup.StartupInfo.cb = Marshal.SizeOf(typeof(StartupInfoEx));
                startup.lpAttributeList = attributes;
                StringBuilder commandLine = new StringBuilder("\"" + executable + "\" " + arguments);
                if (!CreateProcess(executable, commandLine, IntPtr.Zero, IntPtr.Zero, false,
                        ExtendedStartupInfoPresent | CreateNoWindow, IntPtr.Zero,
                        System.IO.Path.GetDirectoryName(executable), ref startup, out process))
                    throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            } finally {
                Marshal.FreeHGlobal(capabilitiesPtr);
            }
            uint wait = WaitForSingleObject(process.hProcess, 30000);
            if (wait != 0) {
                TerminateProcess(process.hProcess, 0xdead);
                throw new TimeoutException("AppContainer child did not exit within 30 seconds.");
            }
            uint exitCode;
            if (!GetExitCodeProcess(process.hProcess, out exitCode))
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            return exitCode;
        } finally {
            if (process.hThread != IntPtr.Zero) CloseHandle(process.hThread);
            if (process.hProcess != IntPtr.Zero) CloseHandle(process.hProcess);
            DeleteProcThreadAttributeList(attributes);
            Marshal.FreeHGlobal(attributes);
        }
    }
}
'@

Add-Type -TypeDefinition $source -Language CSharp

$name = 'CodeggM004-' + [Guid]::NewGuid().ToString('N')
$root = Join-Path $env:RUNNER_TEMP ('codegg-m004-' + [Guid]::NewGuid().ToString('N'))
$readRoot = Join-Path $root 'read-root'
$writeRoot = Join-Path $root 'write-root'
$secretRoot = Join-Path $root 'secret-root'
$outsideRoot = Join-Path $root 'outside-root'
$result = Join-Path $writeRoot 'read-result.txt'
$secret = Join-Path $secretRoot 'credential-fixture.txt'
$outsideWrite = Join-Path $outsideRoot 'outside.txt'
$sid = [IntPtr]::Zero
$profileCreated = $false

try {
    foreach ($path in @($readRoot, $writeRoot, $secretRoot, $outsideRoot)) {
        [void][IO.Directory]::CreateDirectory($path)
    }
    [IO.File]::WriteAllText($secret, 'synthetic-secret-marker')

    $status = [AppContainerProofNative]::CreateAppContainerProfile(
        $name, $name, 'Disposable CodeGG M004 ACL access proof', [IntPtr]::Zero, 0, [ref]$sid)
    if ($status -lt 0 -or $sid -eq [IntPtr]::Zero) {
        throw "CreateAppContainerProfile failed: HRESULT 0x$('{0:X8}' -f $status)"
    }
    $profileCreated = $true
    $sidText = [System.Security.Principal.SecurityIdentifier]::new($sid).Value
    $package = [System.Security.Principal.SecurityIdentifier]::new($sidText)

    $readRule = [System.Security.AccessControl.FileSystemAccessRule]::new(
        $package, [System.Security.AccessControl.FileSystemRights]::ReadAndExecute,
        [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
            [System.Security.AccessControl.InheritanceFlags]::ObjectInherit,
        [System.Security.AccessControl.PropagationFlags]::None,
        [System.Security.AccessControl.AccessControlType]::Allow)
    $writeRule = [System.Security.AccessControl.FileSystemAccessRule]::new(
        $package, [System.Security.AccessControl.FileSystemRights]::Modify,
        [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
            [System.Security.AccessControl.InheritanceFlags]::ObjectInherit,
        [System.Security.AccessControl.PropagationFlags]::None,
        [System.Security.AccessControl.AccessControlType]::Allow)
    $readAcl = Get-Acl -LiteralPath $readRoot
    $readAcl.AddAccessRule($readRule)
    Set-Acl -LiteralPath $readRoot -AclObject $readAcl
    $writeAcl = Get-Acl -LiteralPath $writeRoot
    $writeAcl.AddAccessRule($writeRule)
    Set-Acl -LiteralPath $writeRoot -AclObject $writeAcl
    [IO.File]::WriteAllText((Join-Path $readRoot 'allowed.txt'), 'allowed-read-marker')

    $cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
    $readCode = [AppContainerProofNative]::Run($cmd,
        "/D /C type `"$(Join-Path $readRoot 'allowed.txt')`" `> `"$result`"", $sid)
    if ($readCode -ne 0 -or [IO.File]::ReadAllText($result) -ne 'allowed-read-marker') {
        throw "AppContainer could not read an explicitly allowed workspace fixture (exit $readCode)."
    }
    'allowed workspace read: passed'

    $writeCode = [AppContainerProofNative]::Run($cmd,
        "/D /C echo allowed-write-marker `> `"$(Join-Path $writeRoot 'allowed.txt')`"", $sid)
    if ($writeCode -ne 0 -or -not (Test-Path -LiteralPath (Join-Path $writeRoot 'allowed.txt'))) {
        throw "AppContainer could not write its explicitly allowed scratch fixture (exit $writeCode)."
    }
    'allowed scratch write: passed'

    $secretCode = [AppContainerProofNative]::Run($cmd,
        "/D /C type `"$secret`" `> `"$result`"", $sid)
    if ($secretCode -eq 0 -or [IO.File]::ReadAllText($result).Length -ne 0) {
        throw "AppContainer read the secret fixture without a container ACE (exit $secretCode)."
    }
    'secret read denied: passed'

    $outsideCode = [AppContainerProofNative]::Run($cmd,
        "/D /C echo forbidden `> `"$outsideWrite`"", $sid)
    if (Test-Path -LiteralPath $outsideWrite) {
        throw "AppContainer wrote outside the approved roots (exit $outsideCode)."
    }
    'outside write denied: passed'
} finally {
    if ($profileCreated) {
        $deleteStatus = [AppContainerProofNative]::DeleteAppContainerProfile($name)
        if ($deleteStatus -lt 0) {
            Write-Error "DeleteAppContainerProfile failed: HRESULT 0x$('{0:X8}' -f $deleteStatus)"
        }
    }
    if ($sid -ne [IntPtr]::Zero -and $null -ne $sid) {
        [void][AppContainerProofNative]::FreeSid($sid)
    }
    if (Test-Path -LiteralPath $root) {
        Remove-Item -LiteralPath $root -Recurse -Force
    }
}
