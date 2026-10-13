#include <windows.h>
#include <sddl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>

static void sid_text(PSID sid, wchar_t *out, size_t cap) {
    LPWSTR value = NULL;
    if (sid && ConvertSidToStringSidW(sid, &value)) {
        _snwprintf_s(out, cap, _TRUNCATE, L"%s", value);
        LocalFree(value);
    } else {
        _snwprintf_s(out, cap, _TRUNCATE, L"unavailable");
    }
}

static int write_text(const wchar_t *path, const wchar_t *text) {
    HANDLE file = CreateFileW(path, GENERIC_WRITE, 0, NULL, CREATE_NEW,
                              FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return (int)GetLastError();
    DWORD bytes = (DWORD)(wcslen(text) * sizeof(wchar_t)), written = 0;
    BOOL ok = WriteFile(file, text, bytes, &written, NULL);
    DWORD error = ok && written == bytes ? 0 : GetLastError();
    CloseHandle(file);
    return (int)error;
}

static int child(int argc, wchar_t **argv) {
    if (argc != 7 && argc != 9 && argc != 12) return 40;
    const wchar_t *entry_path = argv[2], *proof_path = argv[3], *nonce = argv[4];
    wchar_t entry[512];
    _snwprintf_s(entry, 512, _TRUNCATE, L"{\"pid\":%lu,\"nonce\":\"%s\",\"phase\":\"entry\"}", GetCurrentProcessId(), nonce);
    int entry_error = write_text(entry_path, entry);
    if (entry_error) return 41;

    HANDLE token = NULL;
    TOKEN_USER *user = NULL;
    TOKEN_GROUPS *groups = NULL;
    TOKEN_MANDATORY_LABEL *integrity = NULL;
    DWORD needed = 0, group_count = 0;
    TOKEN_TYPE token_type = TokenPrimary;
    wchar_t user_sid[256] = L"unavailable", integrity_sid[256] = L"unavailable";
    if (OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token)) {
        GetTokenInformation(token, TokenUser, NULL, 0, &needed);
        user = (TOKEN_USER *)HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, needed);
        if (user && GetTokenInformation(token, TokenUser, user, needed, &needed)) sid_text(user->User.Sid, user_sid, 256);
        needed = 0; GetTokenInformation(token, TokenGroups, NULL, 0, &needed);
        groups = (TOKEN_GROUPS *)HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, needed);
        if (groups && GetTokenInformation(token, TokenGroups, groups, needed, &needed)) group_count = groups->GroupCount;
        needed = 0; GetTokenInformation(token, TokenIntegrityLevel, NULL, 0, &needed);
        integrity = (TOKEN_MANDATORY_LABEL *)HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, needed);
        if (integrity && GetTokenInformation(token, TokenIntegrityLevel, integrity, needed, &needed)) sid_text(integrity->Label.Sid, integrity_sid, 256);
        needed = sizeof(token_type);
        GetTokenInformation(token, TokenType, &token_type, needed, &needed);
    }
    wchar_t group_sids[4096] = L"[";
    if (groups) {
        for (DWORD i = 0; i < group_count; i++) {
            wchar_t current[256];
            sid_text(groups->Groups[i].Sid, current, 256);
            size_t used = wcslen(group_sids);
            _snwprintf_s(group_sids + used, 4096 - used, _TRUNCATE, L"%s\"%s\"", i ? L"," : L"", current);
        }
    }
    wcscat_s(group_sids, 4096, L"]");

    uintptr_t raw = (uintptr_t)_wcstoui64(argv[5], NULL, 10);
    HANDLE inherited = (HANDLE)raw;
    DWORD flags = 0, handle_error = 0, type = FILE_TYPE_UNKNOWN, type_error = 0;
    DWORD read_error = 0, probe_exception = 0;
    BOOL has_flags = FALSE, read_ok = FALSE, same_file = FALSE;
    char secret[128] = {0}; DWORD read_bytes = 0;
    __try {
        has_flags = GetHandleInformation(inherited, &flags);
        handle_error = has_flags ? 0 : GetLastError();
        type = GetFileType(inherited);
        type_error = type == FILE_TYPE_UNKNOWN ? GetLastError() : 0;
        read_ok = has_flags && type == FILE_TYPE_DISK &&
            ReadFile(inherited, secret, sizeof(secret), &read_bytes, NULL);
        read_error = read_ok ? 0 : GetLastError();
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        probe_exception = GetExceptionCode();
        read_ok = FALSE;
    }
    BOOL secret_match = read_ok && read_bytes == 21 && memcmp(secret, "synthetic-c035-secret", 21) == 0;
    if (argc == 9 || argc == 12) {
        BY_HANDLE_FILE_INFORMATION actual = {0};
        DWORD volume_arg = argc == 9 ? 7 : 9;
        DWORD index_arg = argc == 9 ? 8 : 10;
        unsigned long long expected_index = _wcstoui64(argv[index_arg], NULL, 10);
        if (has_flags && GetFileInformationByHandle(inherited, &actual)) {
            unsigned long long actual_index = ((unsigned long long)actual.nFileIndexHigh << 32) | actual.nFileIndexLow;
            same_file = actual.dwVolumeSerialNumber == wcstoul(argv[volume_arg], NULL, 10)
                && actual_index == expected_index;
        }
    }
    HANDLE host_file = INVALID_HANDLE_VALUE;
    DWORD host_path_error = 0, host_path_bytes = 0;
    BOOL host_path_read_ok = FALSE, host_path_secret_match = FALSE;
    char host_path_secret[128] = {0};
    __try {
        host_file = CreateFileW(argv[6], GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING,
                                FILE_ATTRIBUTE_NORMAL, NULL);
        if (host_file != INVALID_HANDLE_VALUE) {
            host_path_read_ok = ReadFile(host_file, host_path_secret, sizeof(host_path_secret),
                                         &host_path_bytes, NULL);
            host_path_error = host_path_read_ok ? 0 : GetLastError();
            host_path_secret_match = host_path_read_ok && host_path_bytes == 21 &&
                memcmp(host_path_secret, "synthetic-c035-secret", 21) == 0;
            CloseHandle(host_file);
        } else {
            host_path_error = GetLastError();
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        host_path_error = GetExceptionCode();
        if (host_file != INVALID_HANDLE_VALUE) CloseHandle(host_file);
    }
    BOOL stdin_ok = FALSE;
    if (argc == 12) {
        char input[256] = {0};
        stdin_ok = fgets(input, sizeof(input), stdin) != NULL;
        if (stdin_ok) {
            input[strcspn(input, "\r\n")] = 0;
            stdin_ok = strcmp(input, "C037_STDIN") == 0;
        }
    }
    wchar_t proof[2048];
    _snwprintf_s(proof, 2048, _TRUNCATE,
        L"{\"pid\":%lu,\"nonce\":\"%s\",\"marker\":\"child-proof\",\"entry_error\":%d,\"handle_present\":%s,\"handle_same_file\":%s,\"handle_inheritable\":%s,\"handle_flags\":%lu,\"handle_error\":%lu,\"file_type\":%lu,\"file_type_error\":%lu,\"read_ok\":%s,\"read_error\":%lu,\"probe_exception\":%lu,\"secret_match\":%s,\"host_path_read_ok\":%s,\"host_path_read_error\":%lu,\"host_path_secret_match\":%s,\"stdin_ok\":%s,\"token_user\":\"%s\",\"group_count\":%lu,\"group_sids\":%s,\"integrity_sid\":\"%s\",\"token_type\":%lu,\"token_observed\":%s}",
        GetCurrentProcessId(), nonce, entry_error, has_flags ? L"true" : L"false", same_file ? L"true" : L"false",
        (has_flags && (flags & HANDLE_FLAG_INHERIT)) ? L"true" : L"false", flags, handle_error, type, type_error,
        read_ok ? L"true" : L"false", read_error, probe_exception, secret_match ? L"true" : L"false",
        host_path_read_ok ? L"true" : L"false", host_path_error, host_path_secret_match ? L"true" : L"false",
        argc == 12 && stdin_ok ? L"true" : L"false", user_sid, group_count, group_sids, integrity_sid, (DWORD)token_type, token ? L"true" : L"false");
    int proof_error = write_text(proof_path, proof);
    if (argc == 12) {
        printf("C037_STDOUT nonce=%ls\n", nonce);
        fwprintf(stderr, L"C037_STDERR nonce=%s\n", nonce);
    }
    if (!proof_error && argc == 12) {
        wchar_t command[2048];
        _snwprintf_s(command, 2048, _TRUNCATE,
            L"\"%s\" --descendant-probe \"%s\" \"%s\" %s \"%s\" %s %s %lu",
            argv[8], argv[7], nonce, argv[5], argv[6], argv[9], argv[10], GetCurrentProcessId());
        STARTUPINFOW si = {0};
        PROCESS_INFORMATION pi = {0};
        si.cb = sizeof(si);
        si.dwFlags = STARTF_USESTDHANDLES;
        si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
        si.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
        si.hStdError = GetStdHandle(STD_ERROR_HANDLE);
        BOOL created = CreateProcessW(argv[8], command, NULL, NULL, TRUE,
            CREATE_NO_WINDOW, NULL, NULL, &si, &pi);
        if (!created) {
            proof_error = 43;
        } else {
            DWORD wait = WaitForSingleObject(pi.hProcess, 10000), code = 0;
            if (wait == WAIT_OBJECT_0) GetExitCodeProcess(pi.hProcess, &code);
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            if (wait != WAIT_OBJECT_0 || code != 0) proof_error = 44;
        }
    }
    if (token) CloseHandle(token);
    if (user) HeapFree(GetProcessHeap(), 0, user);
    if (groups) HeapFree(GetProcessHeap(), 0, groups);
    if (integrity) HeapFree(GetProcessHeap(), 0, integrity);
    return proof_error ? proof_error : 0;
}

static int control(int argc, wchar_t **argv, BOOL inherit) {
    if (argc != 7) return 50;
    HANDLE secret = CreateFileW(argv[2], GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (secret == INVALID_HANDLE_VALUE) return 51;
    BY_HANDLE_FILE_INFORMATION secret_info = {0};
    if (!GetFileInformationByHandle(secret, &secret_info)) { CloseHandle(secret); return 54; }
    unsigned long long secret_index = ((unsigned long long)secret_info.nFileIndexHigh << 32) | secret_info.nFileIndexLow;
    STARTUPINFOEXW si = {0}; PROCESS_INFORMATION pi = {0}; SIZE_T bytes = 0;
    si.StartupInfo.cb = sizeof(si);
    if (inherit) {
        SetHandleInformation(secret, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
        InitializeProcThreadAttributeList(NULL, 1, 0, &bytes);
        si.lpAttributeList = (LPPROC_THREAD_ATTRIBUTE_LIST)HeapAlloc(GetProcessHeap(), 0, bytes);
        if (!si.lpAttributeList || !InitializeProcThreadAttributeList(si.lpAttributeList, 1, 0, &bytes) ||
            !UpdateProcThreadAttribute(si.lpAttributeList, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &secret, sizeof(secret), NULL, NULL)) {
            CloseHandle(secret); return 52;
        }
    } else {
        SetHandleInformation(secret, HANDLE_FLAG_INHERIT, 0);
    }
    wchar_t command[2048];
    _snwprintf_s(command, 2048, _TRUNCATE, L"\"%s\" --child \"%s\" \"%s\" \"%s\" %llu \"%s\" %lu %llu",
        argv[3], argv[4], argv[5], argv[6], (unsigned long long)(uintptr_t)secret, argv[2],
        secret_info.dwVolumeSerialNumber, secret_index);
    BOOL created;
    if (inherit) {
        created = CreateProcessW(argv[3], command, NULL, NULL, TRUE,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW, NULL, NULL, &si.StartupInfo, &pi);
        DeleteProcThreadAttributeList(si.lpAttributeList);
        HeapFree(GetProcessHeap(), 0, si.lpAttributeList);
    } else {
        created = CreateProcessW(argv[3], command, NULL, NULL, FALSE,
            CREATE_NO_WINDOW, NULL, NULL, &si.StartupInfo, &pi);
    }
    if (!created) { DWORD error = GetLastError(); CloseHandle(secret); return (int)(1000 + error); }
    DWORD wait = WaitForSingleObject(pi.hProcess, 15000), code = 0;
    if (wait == WAIT_OBJECT_0) GetExitCodeProcess(pi.hProcess, &code);
    printf("C035_CONTROL pid=%lu wait=%lu exit=%lu handle=%llu\n", pi.dwProcessId, wait, code, (unsigned long long)(uintptr_t)secret);
    CloseHandle(pi.hThread); CloseHandle(pi.hProcess); CloseHandle(secret);
    return wait == WAIT_OBJECT_0 && code == 0 ? 0 : 53;
}

static int descendant_probe(int argc, wchar_t **argv) {
    if (argc != 9) return 80;
    uintptr_t raw = (uintptr_t)_wcstoui64(argv[4], NULL, 10);
    HANDLE inherited = (HANDLE)raw;
    DWORD flags = 0, handle_error = 0, type = FILE_TYPE_UNKNOWN, read_error = 0;
    BOOL has_flags = GetHandleInformation(inherited, &flags);
    if (!has_flags) handle_error = GetLastError();
    type = GetFileType(inherited);
    char bytes[128] = {0}; DWORD read_bytes = 0;
    BOOL read_ok = has_flags && type == FILE_TYPE_DISK && ReadFile(inherited, bytes, sizeof(bytes), &read_bytes, NULL);
    if (!read_ok) read_error = GetLastError();
    BOOL secret_match = read_ok && read_bytes == 21 && memcmp(bytes, "synthetic-c035-secret", 21) == 0;
    BY_HANDLE_FILE_INFORMATION actual = {0};
    unsigned long long expected_index = _wcstoui64(argv[7], NULL, 10);
    unsigned long long actual_index = 0;
    BOOL same_file = has_flags && GetFileInformationByHandle(inherited, &actual)
        && actual.dwVolumeSerialNumber == wcstoul(argv[6], NULL, 10);
    if (same_file) {
        actual_index = ((unsigned long long)actual.nFileIndexHigh << 32) | actual.nFileIndexLow;
        same_file = actual_index == expected_index;
    }
    HANDLE file = CreateFileW(argv[5], GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    DWORD path_error = file == INVALID_HANDLE_VALUE ? GetLastError() : 0, path_bytes = 0;
    BOOL path_read = FALSE, path_match = FALSE;
    char path_content[128] = {0};
    if (file != INVALID_HANDLE_VALUE) {
        path_read = ReadFile(file, path_content, sizeof(path_content), &path_bytes, NULL);
        path_error = path_read ? 0 : GetLastError();
        path_match = path_read && path_bytes == 21 && memcmp(path_content, "synthetic-c035-secret", 21) == 0;
        CloseHandle(file);
    }
    wchar_t proof[1024];
    _snwprintf_s(proof, 1024, _TRUNCATE,
        L"{\"pid\":%lu,\"parent_pid\":%lu,\"nonce\":\"%s\",\"handle_present\":%s,\"handle_same_file\":%s,\"handle_error\":%lu,\"file_type\":%lu,\"read_ok\":%s,\"read_error\":%lu,\"secret_match\":%s,\"host_path_read_ok\":%s,\"host_path_read_error\":%lu,\"host_path_secret_match\":%s}",
        GetCurrentProcessId(), _wtoi(argv[8]), argv[3], has_flags ? L"true" : L"false", same_file ? L"true" : L"false", handle_error,
        type, read_ok ? L"true" : L"false", read_error, secret_match ? L"true" : L"false",
        path_read ? L"true" : L"false", path_error, path_match ? L"true" : L"false");
    return write_text(argv[2], proof) == 0 ? 0 : 81;
}

static int stage_only(int argc, wchar_t **argv) {
    if (argc != 4) return 60;
    wchar_t proof[512];
    _snwprintf_s(proof, 512, _TRUNCATE,
        L"{\"pid\":%lu,\"nonce\":\"%s\",\"marker\":\"stage-only-proof\"}",
        GetCurrentProcessId(), argv[3]);
    return write_text(argv[2], proof) == 0 ? 0 : 61;
}

static int identity(int argc, wchar_t **argv) {
    if (argc != 3) return 90;
    HANDLE file = CreateFileW(argv[2], GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return 91;
    BY_HANDLE_FILE_INFORMATION info = {0};
    BOOL ok = GetFileInformationByHandle(file, &info);
    CloseHandle(file);
    if (!ok) return 92;
    unsigned long long index = ((unsigned long long)info.nFileIndexHigh << 32) | info.nFileIndexLow;
    printf("C037_IDENTITY=%lu,%llu\n", info.dwVolumeSerialNumber, index);
    return 0;
}

static int sleep_probe(int argc, wchar_t **argv) {
    if (argc != 5) return 70;
    wchar_t entry[512];
    _snwprintf_s(entry, 512, _TRUNCATE,
        L"{\"pid\":%lu,\"nonce\":\"%s\",\"phase\":\"entry\"}",
        GetCurrentProcessId(), argv[3]);
    if (write_text(argv[2], entry) != 0) return 71;
    Sleep((DWORD)_wtoi(argv[4]));
    return 0;
}

int wmain(int argc, wchar_t **argv) {
    if (argc > 1 && wcscmp(argv[1], L"--child") == 0) return child(argc, argv);
    if (argc > 1 && wcscmp(argv[1], L"--control-off") == 0) return control(argc, argv, FALSE);
    if (argc > 1 && wcscmp(argv[1], L"--control") == 0) return control(argc, argv, TRUE);
    if (argc > 1 && wcscmp(argv[1], L"--descendant-probe") == 0) return descendant_probe(argc, argv);
    if (argc > 1 && wcscmp(argv[1], L"--stage-only") == 0) return stage_only(argc, argv);
    if (argc > 1 && wcscmp(argv[1], L"--identity") == 0) return identity(argc, argv);
    if (argc > 1 && wcscmp(argv[1], L"--sleep") == 0) return sleep_probe(argc, argv);
    if (argc > 1 && wcscmp(argv[1], L"--exit-before-entry") == 0) return 23;
    return 2;
}
