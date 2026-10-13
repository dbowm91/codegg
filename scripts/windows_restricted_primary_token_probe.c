#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <sddl.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>

#ifdef _MSC_VER
#pragma comment(lib, "Advapi32.lib")
#endif

#define ARRAY_COUNT(array) (sizeof(array) / sizeof((array)[0]))

static BOOL token_user_sid(HANDLE token, LPWSTR output, DWORD output_chars) {
    DWORD required = 0;
    GetTokenInformation(token, TokenUser, NULL, 0, &required);
    if (GetLastError() != ERROR_INSUFFICIENT_BUFFER || required == 0) return FALSE;
    PTOKEN_USER user = (PTOKEN_USER)LocalAlloc(LPTR, required);
    if (!user) return FALSE;
    BOOL ok = GetTokenInformation(token, TokenUser, user, required, &required);
    if (ok) {
        LPWSTR sid = NULL;
        ok = ConvertSidToStringSidW(user->User.Sid, &sid);
        if (ok) {
            wcsncpy_s(output, output_chars, sid, _TRUNCATE);
            LocalFree(sid);
        }
    }
    LocalFree(user);
    return ok;
}

static BOOL has_restricting_sid(HANDLE token, LPCWSTR expected_sid) {
    PSID expected = NULL;
    if (!ConvertStringSidToSidW(expected_sid, &expected)) return FALSE;
    DWORD required = 0;
    GetTokenInformation(token, TokenRestrictedSids, NULL, 0, &required);
    if (GetLastError() != ERROR_INSUFFICIENT_BUFFER || required == 0) {
        LocalFree(expected);
        return FALSE;
    }
    PTOKEN_GROUPS groups = (PTOKEN_GROUPS)LocalAlloc(LPTR, required);
    if (!groups) {
        LocalFree(expected);
        return FALSE;
    }
    BOOL found = FALSE;
    if (GetTokenInformation(token, TokenRestrictedSids, groups, required, &required)) {
        for (DWORD index = 0; index < groups->GroupCount; ++index) {
            if (EqualSid(groups->Groups[index].Sid, expected)) {
                found = TRUE;
                break;
            }
        }
    }
    LocalFree(groups);
    LocalFree(expected);
    return found;
}

static DWORD read_fixture(LPCWSTR path, LPCWSTR expected) {
    HANDLE file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING,
                              FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return GetLastError();
    char actual[256] = {0};
    char expected_utf8[256] = {0};
    int expected_length = WideCharToMultiByte(CP_UTF8, 0, expected, -1,
                                               expected_utf8, (int)ARRAY_COUNT(expected_utf8),
                                               NULL, NULL);
    if (expected_length == 0) {
        DWORD error = GetLastError();
        CloseHandle(file);
        return error;
    }
    DWORD bytes_read = 0;
    BOOL ok = ReadFile(file, actual, (DWORD)sizeof(actual) - 1, &bytes_read, NULL);
    DWORD error = ok ? ERROR_SUCCESS : GetLastError();
    CloseHandle(file);
    if (!ok) return error;
    return strcmp(actual, expected_utf8) == 0 ? ERROR_SUCCESS : ERROR_INVALID_DATA;
}

static DWORD write_stage(LPCWSTR path, DWORD *read_error) {
    HANDLE file = CreateFileW(path, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS,
                              FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return GetLastError();
    static const char payload[] = "stage-ok";
    DWORD written = 0;
    BOOL ok = WriteFile(file, payload, (DWORD)(sizeof(payload) - 1), &written, NULL);
    DWORD error = ok && written == sizeof(payload) - 1 ? ERROR_SUCCESS :
                  (ok ? ERROR_WRITE_FAULT : GetLastError());
    CloseHandle(file);
    *read_error = ERROR_SUCCESS;
    if (error == ERROR_SUCCESS) {
        file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING,
                           FILE_ATTRIBUTE_NORMAL, NULL);
        if (file == INVALID_HANDLE_VALUE) {
            *read_error = GetLastError();
        } else {
            char actual[16] = {0};
            DWORD read = 0;
            static const char expected[] = "stage-ok";
            BOOL read_ok = ReadFile(file, actual, sizeof(actual), &read, NULL);
            if (!read_ok) *read_error = GetLastError();
            else if (read != (DWORD)(sizeof(expected) - 1) || memcmp(actual, expected, sizeof(expected) - 1) != 0)
                *read_error = ERROR_INVALID_DATA;
            CloseHandle(file);
        }
    }
    DeleteFileW(path);
    return error;
}

static DWORD run_command(LPCWSTR application, LPCWSTR command_line, DWORD *exit_code) {
    STARTUPINFOW startup = {0};
    PROCESS_INFORMATION process = {0};
    startup.cb = (DWORD)sizeof(startup);
    startup.dwFlags = STARTF_USESHOWWINDOW;
    startup.wShowWindow = SW_HIDE;
    WCHAR mutable_command[32768];
    if (wcsncpy_s(mutable_command, ARRAY_COUNT(mutable_command), command_line, _TRUNCATE) != 0)
        return ERROR_INSUFFICIENT_BUFFER;
    if (!CreateProcessW(application, mutable_command, NULL, NULL, FALSE, CREATE_NO_WINDOW,
                        NULL, NULL, &startup, &process)) return GetLastError();
    DWORD wait = WaitForSingleObject(process.hProcess, 5000);
    DWORD error = ERROR_SUCCESS;
    if (wait == WAIT_TIMEOUT) {
        TerminateProcess(process.hProcess, ERROR_TIMEOUT);
        WaitForSingleObject(process.hProcess, 1000);
        error = ERROR_TIMEOUT;
    } else if (wait != WAIT_OBJECT_0) {
        error = GetLastError();
    } else if (!GetExitCodeProcess(process.hProcess, exit_code)) {
        error = GetLastError();
    }
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
    return error;
}

static BOOL write_result(LPCWSTR path, LPCWSTR user_sid, BOOL restricted_sid_seen,
                         DWORD broad_error, DWORD private_error, DWORD stage_error,
                         DWORD stage_read_error, DWORD shell_error, DWORD shell_exit,
                         DWORD git_error, DWORD git_exit) {
    HANDLE file = CreateFileW(path, GENERIC_WRITE, FILE_SHARE_READ, NULL, CREATE_ALWAYS,
                              FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return FALSE;
    char json[1024];
    int length = _snprintf_s(json, sizeof(json), _TRUNCATE,
        "{\"token_user\":\"%ls\",\"token_type\":\"primary\","
        "\"limiting_sid_seen_in_child\":%s,\"broad_read_win32_error\":%lu,"
        "\"private_read_win32_error\":%lu,\"stage_write_win32_error\":%lu,"
        "\"stage_read_win32_error\":%lu,\"shell_start_win32_error\":%lu,"
        "\"shell_exit_code\":%lu,\"git_start_win32_error\":%lu,\"git_exit_code\":%lu}",
        user_sid, restricted_sid_seen ? "true" : "false", broad_error, private_error,
        stage_error, stage_read_error, shell_error, shell_exit, git_error, git_exit);
    DWORD written = 0;
    BOOL ok = length > 0 && WriteFile(file, json, (DWORD)length, &written, NULL) &&
              written == (DWORD)length;
    CloseHandle(file);
    return ok;
}

int wmain(int argc, wchar_t **argv) {
    if (argc != 8) return 64;
    LPCWSTR broad_path = argv[1];
    LPCWSTR private_path = argv[2];
    LPCWSTR stage_path = argv[3];
    LPCWSTR limiting_sid = argv[4];
    LPCWSTR expected_broad = argv[5];
    LPCWSTR result_path = argv[6];
    LPCWSTR git_exe = argv[7];

    HANDLE token = NULL;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token)) return 65;
    TOKEN_TYPE token_type = TokenImpersonation;
    DWORD returned = 0;
    BOOL token_ok = GetTokenInformation(token, TokenType, &token_type, sizeof(token_type),
                                        &returned) && token_type == TokenPrimary;
    WCHAR user_sid[184] = {0};
    BOOL sid_ok = token_user_sid(token, user_sid, (DWORD)ARRAY_COUNT(user_sid));
    BOOL restricting_sid_seen = has_restricting_sid(token, limiting_sid);
    CloseHandle(token);
    if (!token_ok || !sid_ok) return 66;

    DWORD broad_error = read_fixture(broad_path, expected_broad);
    DWORD private_error = read_fixture(private_path, L"synthetic-c027-private-secret");
    DWORD stage_read_error = ERROR_SUCCESS;
    DWORD stage_error = write_stage(stage_path, &stage_read_error);
    DWORD shell_exit = MAXDWORD, git_exit = MAXDWORD;
    DWORD shell_error = ERROR_NOT_READY, git_error = ERROR_NOT_READY;
    if (restricting_sid_seen && broad_error == ERROR_ACCESS_DENIED &&
        stage_error == ERROR_SUCCESS && stage_read_error == ERROR_SUCCESS) {
        WCHAR system_root[MAX_PATH] = {0};
        DWORD root_length = GetEnvironmentVariableW(L"SystemRoot", system_root, (DWORD)ARRAY_COUNT(system_root));
        if (root_length > 0 && root_length < (DWORD)ARRAY_COUNT(system_root)) {
            WCHAR command_exe[MAX_PATH] = {0};
            swprintf_s(command_exe, ARRAY_COUNT(command_exe), L"%ls\\System32\\cmd.exe", system_root);
            WCHAR shell_command[32768] = {0};
            swprintf_s(shell_command, ARRAY_COUNT(shell_command), L"\"%ls\" /d /c ver", command_exe);
            shell_error = run_command(command_exe, shell_command, &shell_exit);
        } else shell_error = ERROR_ENVVAR_NOT_FOUND;
        WCHAR git_command[32768] = {0};
        swprintf_s(git_command, ARRAY_COUNT(git_command), L"\"%ls\" --version", git_exe);
        git_error = run_command(git_exe, git_command, &git_exit);
    }
    if (!write_result(result_path, user_sid, restricting_sid_seen, broad_error,
                      private_error, stage_error, stage_read_error, shell_error,
                      shell_exit, git_error, git_exit)) return 67;
    return 0;
}
