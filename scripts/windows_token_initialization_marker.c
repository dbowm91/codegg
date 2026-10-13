#define WIN32_LEAN_AND_MEAN
#include <windows.h>

static DWORD write_marker(const char *name, const char *value) {
    HANDLE file = CreateFileA(name, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
                              NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return GetLastError();
    DWORD written = 0;
    BOOL ok = WriteFile(file, value, (DWORD)(lstrlenA(value)), &written, NULL);
    DWORD error = ok && written == (DWORD)lstrlenA(value) ? ERROR_SUCCESS : GetLastError();
    CloseHandle(file);
    return error;
}

static DWORD write_context(void) {
    HANDLE file = CreateFileW(L"context.txt", FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
                              NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE) return GetLastError();
    WCHAR value[32768];
    DWORD error = ERROR_SUCCESS;
    const WCHAR *names[] = {L"USERNAME", L"USERPROFILE", L"TEMP", L"TMP", L"PATH"};
    for (DWORD i = 0; i < ARRAYSIZE(names); ++i) {
        DWORD length = GetEnvironmentVariableW(names[i], value, ARRAYSIZE(value));
        if (length >= ARRAYSIZE(value)) { error = ERROR_BUFFER_OVERFLOW; break; }
        DWORD written = 0;
        if (!WriteFile(file, names[i], (DWORD)(lstrlenW(names[i]) * sizeof(WCHAR)), &written, NULL) ||
            !WriteFile(file, L"=", sizeof(WCHAR), &written, NULL) ||
            (length && !WriteFile(file, value, (DWORD)(length * sizeof(WCHAR)), &written, NULL)) ||
            !WriteFile(file, L"\r\n", 2 * sizeof(WCHAR), &written, NULL)) {
            error = GetLastError(); break;
        }
    }
    DWORD desktopLength = 0;
    HDESK desktop = GetThreadDesktop(GetCurrentThreadId());
    if (!desktop || !GetUserObjectInformationW(desktop, UOI_NAME, value, sizeof(value), &desktopLength)) {
        error = GetLastError();
    } else {
        DWORD written = 0;
        if (!WriteFile(file, L"DESKTOP=", 8 * sizeof(WCHAR), &written, NULL) ||
            !WriteFile(file, value, desktopLength - sizeof(WCHAR), &written, NULL) ||
            !WriteFile(file, L"\r\n", 2 * sizeof(WCHAR), &written, NULL)) error = GetLastError();
    }
    CloseHandle(file);
    return error;
}

#if defined(C028_NO_CRT)
void WINAPI C028NoCrtEntry(void) {
    DWORD error = write_marker("entry.marker", "native-entry\n");
    if (error == ERROR_SUCCESS) error = write_context();
    if (error == ERROR_SUCCESS) Sleep(500);
    if (error == ERROR_SUCCESS) error = write_marker("complete.marker", "complete\n");
    ExitProcess(error == ERROR_SUCCESS ? 0 : error);
}
#else
int main(void) {
    DWORD error = write_marker("entry.marker", "crt-main\n");
    if (error == ERROR_SUCCESS) error = write_context();
    if (error == ERROR_SUCCESS) Sleep(500);
    if (error == ERROR_SUCCESS) error = write_marker("complete.marker", "complete\n");
    return (int)error;
}
#endif
