/* C044 native first-instruction probe (test only, Windows).
 *
 * Build (disposable consented host only):
 *   cl.exe /MT /O1 /GS /sdl /W4 /DUNICODE /D_UNICODE native_probe.c /link /out:native_probe.exe
 * (/MT statically links the CRT: under the restricted token the loader
 * then needs only the probe file itself plus kernel32/advapi32 KnownDLLs,
 * never a system32/WinSxS CRT open the restricting SID is not granted.)
 * Record the resulting SHA-256 in the run evidence; the binary lives in
 * the runner-owned immutable directory and is removed with the fixtures.
 *
 * Protocol: argv[1] = entry_path, argv[2] = proof_path, argv[3] = nonce,
 * argv[4] = broad_read_path, argv[5] = host_write_path,
 * argv[6..] = extra alias read paths (junction/hardlink/ADS/case/
 * extended-path; at most 8 are recorded).
 * The probe writes the entry marker FIRST, before any interpreter, CRT
 * locale, or tool loads beyond what the loader already mapped. A missing
 * entry marker means loader/privilege/environment failure, never a
 * read-denial result.
 */

#include <windows.h>
#include <sddl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>

static int write_text(const wchar_t *path, const wchar_t *text) {
    HANDLE file = CreateFileW(path, GENERIC_WRITE, 0, NULL, CREATE_NEW,
                              FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) {
        return (int)GetLastError();
    }
    DWORD bytes = (DWORD)(wcslen(text) * sizeof(wchar_t));
    DWORD written = 0;
    BOOL ok = WriteFile(file, text, bytes, &written, NULL);
    DWORD error = (ok && written == bytes) ? 0 : GetLastError();
    CloseHandle(file);
    return (int)error;
}

static void sid_text(PSID sid, wchar_t *out, size_t cap) {
    LPWSTR value = NULL;
    if (sid != NULL && ConvertSidToStringSidW(sid, &value)) {
        _snwprintf_s(out, cap, _TRUNCATE, L"%s", value);
        LocalFree(value);
    } else {
        _snwprintf_s(out, cap, _TRUNCATE, L"unavailable");
    }
}

static int probe_read(const wchar_t *path) {
    HANDLE file =
        CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, 0, NULL);
    if (file == INVALID_HANDLE_VALUE) {
        return (int)GetLastError();
    }
    char scratch[64];
    DWORD read = 0;
    (void)ReadFile(file, scratch, sizeof(scratch), &read, NULL);
    CloseHandle(file);
    return 0;
}

static int probe_write(const wchar_t *path) {
    HANDLE file = CreateFileW(path, GENERIC_WRITE, 0, NULL, CREATE_NEW,
                              FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) {
        return (int)GetLastError();
    }
    DWORD written = 0;
    const char body[] = "c044";
    BOOL ok = WriteFile(file, body, (DWORD)sizeof(body), &written, NULL);
    DWORD error = ok ? 0 : GetLastError();
    CloseHandle(file);
    return (int)error;
}

int wmain(int argc, wchar_t **argv) {
    if (argc < 6) {
        return 40;
    }
    const wchar_t *entry_path = argv[1];
    const wchar_t *proof_path = argv[2];
    const wchar_t *nonce = argv[3];
    const wchar_t *broad_path = argv[4];
    const wchar_t *host_path = argv[5];

    /* First instruction: nonce-bound entry proof. */
    wchar_t entry[512];
    _snwprintf_s(entry, 512, _TRUNCATE,
                 L"{\"pid\":%lu,\"nonce\":\"%s\",\"phase\":\"entry\"}",
                 GetCurrentProcessId(), nonce);
    int entry_error = write_text(entry_path, entry);
    if (entry_error != 0) {
        return 41;
    }

    /* Token identity. */
    HANDLE token = NULL;
    wchar_t user_sid[256] = L"unavailable";
    DWORD restricting = 0;
    if (OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token)) {
        DWORD needed = 0;
        TOKEN_USER *user = NULL;
        TOKEN_GROUPS *groups = NULL;
        GetTokenInformation(token, TokenUser, NULL, 0, &needed);
        user = (TOKEN_USER *)HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, needed);
        if (user != NULL &&
            GetTokenInformation(token, TokenUser, user, needed, &needed)) {
            sid_text(user->User.Sid, user_sid, 256);
        }
        needed = 0;
        GetTokenInformation(token, TokenGroups, NULL, 0, &needed);
        groups = (TOKEN_GROUPS *)HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, needed);
        if (groups != NULL &&
            GetTokenInformation(token, TokenGroups, groups, needed, &needed)) {
            for (DWORD i = 0; i < groups->GroupCount; i++) {
                if ((groups->Groups[i].Attributes &
                     SE_GROUP_USE_FOR_DENY_ONLY) != 0) {
                    restricting++;
                }
            }
        }
        if (user != NULL) {
            HeapFree(GetProcessHeap(), 0, user);
        }
        if (groups != NULL) {
            HeapFree(GetProcessHeap(), 0, groups);
        }
        CloseHandle(token);
    }

    int broad_result = probe_read(broad_path);
    int host_result = probe_write(host_path);

    /* Extra alias routes: each read-probed, Win32 error recorded in
     * order. Fixed cap of 8 keeps the proof bounded and avoids any
     * truncation arithmetic in the JSON buffer below. */
    int alias_errors[8];
    int alias_count = 0;
    for (int i = 6; i < argc && alias_count < 8; i++) {
        alias_errors[alias_count++] = probe_read(argv[i]);
    }
    wchar_t alias_list[128] = L"";
    for (int i = 0; i < alias_count; i++) {
        wchar_t cell[16];
        _snwprintf_s(cell, 16, _TRUNCATE, L"%s%d", i == 0 ? L"" : L",", alias_errors[i]);
        wcscat_s(alias_list, 128, cell);
    }

    wchar_t proof[1536];
    _snwprintf_s(
        proof, 1536, _TRUNCATE,
        L"{\"pid\":%lu,\"nonce\":\"%s\",\"phase\":\"token\",\"user\":\"%s\","
        L"\"restricting_groups\":%lu,\"broad_read_error\":%d,\"host_write_error\":%d,"
        L"\"alias_count\":%d,\"alias_errors\":[%s]}",
        GetCurrentProcessId(), nonce, user_sid, restricting, broad_result,
        host_result, alias_count, alias_list);
    int proof_error = write_text(proof_path, proof);
    return proof_error == 0 ? 0 : 42;
}
