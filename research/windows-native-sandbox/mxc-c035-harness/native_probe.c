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
    if (argc != 6) return 40;
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
    BOOL has_flags = FALSE, read_ok = FALSE;
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
    wchar_t proof[2048];
    _snwprintf_s(proof, 2048, _TRUNCATE,
        L"{\"pid\":%lu,\"nonce\":\"%s\",\"marker\":\"child-proof\",\"entry_error\":%d,\"handle_present\":%s,\"handle_inheritable\":%s,\"handle_flags\":%lu,\"handle_error\":%lu,\"file_type\":%lu,\"file_type_error\":%lu,\"read_ok\":%s,\"read_error\":%lu,\"probe_exception\":%lu,\"secret_match\":%s,\"token_user\":\"%s\",\"group_count\":%lu,\"group_sids\":%s,\"integrity_sid\":\"%s\",\"token_type\":%lu,\"token_observed\":%s}",
        GetCurrentProcessId(), nonce, entry_error, has_flags ? L"true" : L"false",
        (has_flags && (flags & HANDLE_FLAG_INHERIT)) ? L"true" : L"false", flags, handle_error, type, type_error,
        read_ok ? L"true" : L"false", read_error, probe_exception, secret_match ? L"true" : L"false", user_sid,
        group_count, group_sids, integrity_sid, (DWORD)token_type, token ? L"true" : L"false");
    int proof_error = write_text(proof_path, proof);
    if (token) CloseHandle(token);
    if (user) HeapFree(GetProcessHeap(), 0, user);
    if (groups) HeapFree(GetProcessHeap(), 0, groups);
    if (integrity) HeapFree(GetProcessHeap(), 0, integrity);
    return proof_error ? 42 : 0;
}

static int control(int argc, wchar_t **argv) {
    if (argc != 7) return 50;
    HANDLE secret = CreateFileW(argv[2], GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (secret == INVALID_HANDLE_VALUE) return 51;
    SetHandleInformation(secret, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
    STARTUPINFOEXW si = {0}; PROCESS_INFORMATION pi = {0}; SIZE_T bytes = 0;
    si.StartupInfo.cb = sizeof(si);
    InitializeProcThreadAttributeList(NULL, 1, 0, &bytes);
    si.lpAttributeList = (LPPROC_THREAD_ATTRIBUTE_LIST)HeapAlloc(GetProcessHeap(), 0, bytes);
    if (!si.lpAttributeList || !InitializeProcThreadAttributeList(si.lpAttributeList, 1, 0, &bytes) ||
        !UpdateProcThreadAttribute(si.lpAttributeList, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &secret, sizeof(secret), NULL, NULL)) {
        CloseHandle(secret); return 52;
    }
    wchar_t command[2048];
    _snwprintf_s(command, 2048, _TRUNCATE, L"\"%s\" --child \"%s\" \"%s\" \"%s\" %llu",
        argv[3], argv[4], argv[5], argv[6], (unsigned long long)(uintptr_t)secret);
    BOOL created = CreateProcessW(argv[3], command, NULL, NULL, TRUE,
        EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW, NULL, NULL, &si.StartupInfo, &pi);
    DeleteProcThreadAttributeList(si.lpAttributeList);
    HeapFree(GetProcessHeap(), 0, si.lpAttributeList);
    if (!created) { DWORD error = GetLastError(); CloseHandle(secret); return (int)(1000 + error); }
    DWORD wait = WaitForSingleObject(pi.hProcess, 15000), code = 0;
    if (wait == WAIT_OBJECT_0) GetExitCodeProcess(pi.hProcess, &code);
    printf("C035_CONTROL pid=%lu wait=%lu exit=%lu handle=%llu\n", pi.dwProcessId, wait, code, (unsigned long long)(uintptr_t)secret);
    CloseHandle(pi.hThread); CloseHandle(pi.hProcess); CloseHandle(secret);
    return wait == WAIT_OBJECT_0 && code == 0 ? 0 : 53;
}

int wmain(int argc, wchar_t **argv) {
    if (argc > 1 && wcscmp(argv[1], L"--child") == 0) return child(argc, argv);
    if (argc > 1 && wcscmp(argv[1], L"--control") == 0) return control(argc, argv);
    if (argc > 1 && wcscmp(argv[1], L"--exit-before-entry") == 0) return 23;
    return 2;
}
