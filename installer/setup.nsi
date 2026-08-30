Unicode true
RequestExecutionLevel user
ManifestDPIAware true
CRCCheck on
SetCompressor /SOLID lzma
AutoCloseWindow false
ShowInstDetails nevershow

!include "nsDialogs.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "WinMessages.nsh"
!include "StrFunc.nsh"

${StrCase}

; The first custom page needs nsDialogs before the embedded 283 MB MSI.
; Reserving it keeps the solid archive from seeking through the MSI at startup.
ReserveFile /plugin nsDialogs.dll

!ifndef APP_VERSION
  !define APP_VERSION "1.0.1"
!endif
!ifndef MSI_PATH
  !error "MSI_PATH is required"
!endif
!ifndef WORKER_PATH
  !error "WORKER_PATH is required"
!endif
!ifndef ASSET_DIR
  !error "ASSET_DIR is required"
!endif
!ifndef OUTPUT_PATH
  !define OUTPUT_PATH "MediaDrop-Setup-${APP_VERSION}.exe"
!endif
!define EXTENSION_CONNECTED_EVENT "Local\MediaDrop.ExtensionSetup.Connected.v1"
!define SETUP_MUTEX "Local\MediaDrop.Setup.UI.v1"

VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "MediaDrop"
VIAddVersionKey /LANG=1033 "FileDescription" "MediaDrop offline installer"
VIAddVersionKey /LANG=1033 "CompanyName" "Depthsss"
VIAddVersionKey /LANG=1033 "LegalCopyright" "Copyright 2026 Depthsss"
VIAddVersionKey /LANG=1033 "FileVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=1033 "OriginalFilename" "MediaDrop-Setup-${APP_VERSION}.exe"

Name "MediaDrop ${APP_VERSION}"
Caption "MediaDrop Kurulum"
OutFile "${OUTPUT_PATH}"
InstallDir "$PROGRAMFILES64\MediaDrop"
BrandingText "MediaDrop"

!ifdef APP_ICON
  Icon "${APP_ICON}"
!endif

Page custom SetupPage
Page instfiles

Var Dialog
Var Background
Var BackgroundImage
Var CurrentScreen
Var PreviewScreen
!ifdef UI_TEST_MODE
Var PreviewHidden
Var PreviewOffscreen
Var PreviewActionLog
Var PreviewHoverTest
Var TestWorkerScenario
!endif
!ifdef PREVIEW_MODE
Var SelfTestPath
!endif
Var Dragging
Var MotionEnabled
Var HoverVisual
Var HoverBitmap
Var HoverTarget
Var HoverX
Var HoverY
Var HoverWidth
Var HoverHeight
Var HoverProgress
Var HoverMouseX
Var HoverMouseY
Var ToggleTarget

Var MinimizeHit
Var CloseHit
Var StartHit
Var CancelHit
Var SummaryHit
Var FinishHit
Var LogHit
Var GiveUpHit
Var RetryHit
Var FilesRetryHit
Var FilesContinueHit
Var FilesCancelHit
Var WelcomeToggleHit
Var LaunchToggleHit
Var ExtensionToggleHit

Var WelcomeToggle
Var WelcomeToggleImage
Var LaunchToggle
Var LaunchToggleImage
Var ExtensionToggle
Var ExtensionToggleImage
Var LaunchApp
Var ExtensionSetup
Var ExtensionHandled
Var ExtensionConnected
Var ExtensionEvent
Var ExtensionPath
!ifndef UI_TEST_MODE
Var ExtensionRoot
Var MediaDropExe
!endif
Var DefaultBrowserId
!ifndef UI_TEST_MODE
Var DefaultProgId
!endif
Var BrowserCount
Var SelectedBrowserSlot
Var SelectedBrowserId
Var SelectedBrowserLabel
Var SelectedBrowserExe
Var SelectedBrowserPage
Var ExtensionBrowserOpened
Var ExtensionPathCopied

Var BrowserId0
Var BrowserLabel0
Var BrowserExe0
Var BrowserPage0
Var BrowserId1
Var BrowserLabel1
Var BrowserExe1
Var BrowserPage1
Var BrowserId2
Var BrowserLabel2
Var BrowserExe2
Var BrowserPage2
Var BrowserId3
Var BrowserLabel3
Var BrowserExe3
Var BrowserPage3
!ifndef UI_TEST_MODE
Var OperaGxExe
Var OperaExe
Var ChromeExe
Var EdgeExe
!endif

Var ExtensionLaterHit
Var ExtensionCopyHit
Var ExtensionRevealHit
Var ExtensionPrimaryHit
Var ExtensionBrowserHit0
Var ExtensionBrowserHit1
Var ExtensionBrowserHit2
Var ExtensionBrowserHit3
Var ExtensionBrowserMask0
Var ExtensionBrowserMask1
Var ExtensionBrowserMask2
Var ExtensionBrowserMask3
Var ExtensionStep0
Var ExtensionStep1
Var ExtensionStep2
Var ExtensionStatusDot
Var ExtensionStatusTitle
Var ExtensionStatusDetail

Var Logo
Var LogoImage
Var Progress
Var ProgressFill
Var ProgressNumber
Var ProgressCurrent
Var LastStage
Var StageDot0
Var StageDot1
Var StageDot2
Var StageDot3
Var StageName0
Var StageName1
Var StageName2
Var StageName3
Var StageState0
Var StageState1
Var StageState2
Var StageState3

Var ErrorLead
Var ErrorTitle
Var ErrorDetail
Var InstallExitCode
Var InstallTick
Var InstallLog
Var InstallFailureKind
Var ExitRequested
Var PreviewAnimating
Var InstallActive
Var BrokerStarted
Var SetupMutex
Var StartedElevated
Var CancelRequested
Var SessionId
Var SessionRoot
Var SessionDir
Var StatusPath
Var CommandPath
Var CommandSequence
Var StatusSequence
Var StatusState
Var StatusPhase
Var StatusAction
Var StatusResultKind
Var StatusWin32Code
Var StatusMsiCode
Var StatusLogPath
Var StatusChanged
Var SilentMode
Var SessionErrorCode

Var FontNormal
Var FontSemibold
Var FontSmall
Var FontPercent

!macro CreateHit HANDLE X Y W H TEXT COLOR CALLBACK
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${WS_TABSTOP}|${SS_NOTIFY} ${WS_EX_TRANSPARENT} ${X} ${Y} ${W} ${H} ""
  Pop ${HANDLE}
  SetCtlColors ${HANDLE} ${COLOR} transparent
  ${NSD_OnClick} ${HANDLE} ${CALLBACK}
!macroend

!macro CreateTextButton HANDLE X Y W H TEXT FOREGROUND BACKGROUND CALLBACK
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${WS_TABSTOP}|${SS_NOTIFY}|${SS_CENTER}|${SS_CENTERIMAGE} 0 ${X} ${Y} ${W} ${H} "${TEXT}"
  Pop ${HANDLE}
  SetCtlColors ${HANDLE} ${FOREGROUND} ${BACKGROUND}
  SendMessage ${HANDLE} ${WM_SETFONT} $FontSemibold 1
  ${NSD_OnClick} ${HANDLE} ${CALLBACK}
!macroend

!macro CreateOverlayButton HANDLE X Y W H TEXT FOREGROUND CALLBACK
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${WS_TABSTOP}|${SS_NOTIFY}|${SS_CENTER}|${SS_CENTERIMAGE} ${WS_EX_TRANSPARENT} ${X} ${Y} ${W} ${H} "${TEXT}"
  Pop ${HANDLE}
  SetCtlColors ${HANDLE} ${FOREGROUND} transparent
  SendMessage ${HANDLE} ${WM_SETFONT} $FontSemibold 1
  ${NSD_OnClick} ${HANDLE} ${CALLBACK}
!macroend

!macro HideControl HANDLE
  ShowWindow ${HANDLE} ${SW_HIDE}
!macroend

!macro ShowControl HANDLE
  ShowWindow ${HANDLE} ${SW_SHOW}
!macroend

!macro PrepareOverlay HANDLE
  ${NSD_RemoveStyle} ${HANDLE} ${WS_CLIPSIBLINGS}
  System::Call 'user32::SetWindowPos(p ${HANDLE}, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
!macroend

!macro MatchHoverRect TARGET X Y RIGHT BOTTOM
  ${If} $3 == ""
  ${AndIf} $HoverMouseX >= ${X}
  ${AndIf} $HoverMouseX < ${RIGHT}
  ${AndIf} $HoverMouseY >= ${Y}
  ${AndIf} $HoverMouseY < ${BOTTOM}
    StrCpy $3 "${TARGET}"
    IntOp $4 ${X} - 2
    IntOp $5 ${Y} - 1
    IntOp $6 ${RIGHT} - ${X}
    IntOp $6 $6 + 4
    IntOp $7 ${BOTTOM} - ${Y}
    IntOp $7 $7 + 2
  ${EndIf}
!macroend

!macro RecordPreviewAction ACTION
!ifdef UI_TEST_MODE
  ${If} $PreviewActionLog != ""
    FileOpen $9 "$PreviewActionLog" a
    FileSeek $9 0 END
    FileWrite $9 "${ACTION}$\r$\n"
    FileClose $9
  ${EndIf}
!endif
!macroend

Function .onInit
  StrCpy $SetupMutex 0
  System::Call 'kernel32::CreateMutexW(p 0, i 0, w "${SETUP_MUTEX}") p .r0 ?e'
  Pop $1
  StrCpy $SetupMutex $0
  ${If} $SetupMutex != 0
  ${AndIf} $1 == 183
    System::Call 'user32::FindWindowW(w "#32770", w "MediaDrop Kurulum") p .r2'
    ${If} $2 != 0
      System::Call 'user32::ShowWindow(p $2, i 9) i .r3'
      System::Call 'user32::SetForegroundWindow(p $2) i .r3'
    ${EndIf}
    System::Call 'kernel32::CloseHandle(p $SetupMutex) i .r2'
    StrCpy $SetupMutex 0
    SetErrorLevel 0
    Abort
  ${EndIf}

  StrCpy $StartedElevated 0
  System::Call 'kernel32::GetCurrentProcess() p .r0'
  System::Call 'advapi32::OpenProcessToken(p $0, i 0x8, *p .r1) i .r2'
  ${If} $2 != 0
    System::Call 'advapi32::GetTokenInformation(p $1, i 20, *i .r3, i 4, *i .r4) i .r2'
    ${If} $2 != 0
      StrCpy $StartedElevated $3
    ${EndIf}
    System::Call 'kernel32::CloseHandle(p $1) i .r2'
  ${EndIf}

  StrCpy $MotionEnabled 1
  System::Call 'user32::SystemParametersInfoW(i 0x1042, i 0, *i .r0, i 0) i .r1'
  ${If} $1 != 0
    StrCpy $MotionEnabled $0
  ${EndIf}
  StrCpy $HoverTarget ""
  StrCpy $HoverProgress 0
  StrCpy $ToggleTarget ""
  StrCpy $LaunchApp 1
  StrCpy $ExtensionSetup 0
  StrCpy $ExtensionHandled 0
  StrCpy $ExtensionConnected 0
  StrCpy $ExtensionEvent 0
  StrCpy $BrowserCount 0
  StrCpy $SelectedBrowserSlot -1
  StrCpy $ExtensionBrowserOpened 0
  StrCpy $ExtensionPathCopied 0
  StrCpy $PreviewScreen "welcome"
  StrCpy $InstallFailureKind ""
  StrCpy $ExitRequested 0
  StrCpy $InstallActive 0
  StrCpy $BrokerStarted 0
  StrCpy $CancelRequested 0
  StrCpy $CommandSequence 0
  StrCpy $StatusSequence 0
  StrCpy $StatusChanged 0
  StrCpy $SilentMode 0
  StrCpy $SessionErrorCode 0
  StrCpy $Progress 0
  StrCpy $LastStage -1

!ifdef UI_TEST_MODE
  ${GetParameters} $0
  StrCpy $PreviewHidden 0
  StrCpy $PreviewOffscreen 0
  StrCpy $PreviewActionLog ""
  StrCpy $PreviewHoverTest ""
  StrCpy $TestWorkerScenario "success"
  ${GetOptions} "$0" "/HIDDENUITEST=" $1
  ${If} $1 == "1"
    StrCpy $PreviewHidden 1
    ShowWindow $HWNDPARENT ${SW_HIDE}
  ${EndIf}
  ${GetOptions} "$0" "/OFFSCREENUITEST=" $1
  ${If} $1 == "1"
    StrCpy $PreviewOffscreen 1
  ${EndIf}
  ${GetOptions} "$0" "/UIACTIONLOG=" $PreviewActionLog
  ${GetOptions} "$0" "/HOVERUITEST=" $PreviewHoverTest
  ${GetOptions} "$0" "/WORKERSCENARIO=" $1
  ${If} $1 != ""
    StrCpy $TestWorkerScenario $1
  ${EndIf}
!ifdef PREVIEW_MODE
  ${GetOptions} "$0" "/UISELFTEST=" $SelfTestPath
  ${If} $SelfTestPath != ""
    InitPluginsDir
    SetOutPath "$PLUGINSDIR"
    File /oname=ui-contract.json "${ASSET_DIR}\ui-contract.json"
    ClearErrors
    CopyFiles /SILENT "$PLUGINSDIR\ui-contract.json" "$SelfTestPath"
    ${If} ${Errors}
      SetErrorLevel 2
    ${Else}
      SetErrorLevel 0
    ${EndIf}
    Quit
  ${EndIf}

  ${GetOptions} "$0" "/SCREEN=" $1
  ${If} $1 == "welcome"
  ${OrIf} $1 == "installing"
  ${OrIf} $1 == "extension"
  ${OrIf} $1 == "done"
  ${OrIf} $1 == "error"
    StrCpy $PreviewScreen $1
  ${EndIf}
!endif
!endif
FunctionEnd

Function .onGUIInit
  ShowWindow $HWNDPARENT ${SW_HIDE}
  !insertmacro RecordPreviewAction "startup_hidden"
FunctionEnd

Function ExtractUiAssets
  InitPluginsDir
  SetOutPath "$PLUGINSDIR\ui"
  File /oname=screen-welcome.bmp "${ASSET_DIR}\screen-welcome.bmp"
  File /oname=screen-installing.bmp "${ASSET_DIR}\screen-installing.bmp"
  File /oname=screen-extension.bmp "${ASSET_DIR}\screen-extension.bmp"
  File /oname=screen-done.bmp "${ASSET_DIR}\screen-done.bmp"
  File /oname=screen-error.bmp "${ASSET_DIR}\screen-error.bmp"
  File "${ASSET_DIR}\toggle-*.bmp"
  File "${ASSET_DIR}\hover-*.bmp"
  File /oname=InstrumentSans-Regular.ttf "${ASSET_DIR}\InstrumentSans-Regular.ttf"
  File /oname=InstrumentSans-Medium.ttf "${ASSET_DIR}\InstrumentSans-Medium.ttf"
  File /oname=InstrumentSans-SemiBold.ttf "${ASSET_DIR}\InstrumentSans-SemiBold.ttf"
  File /oname=InstrumentSans-Bold.ttf "${ASSET_DIR}\InstrumentSans-Bold.ttf"
  SetOutPath "$PLUGINSDIR\ui\logo"
  File "${ASSET_DIR}\logo\*.bmp"
FunctionEnd

Function ExtractMsi
  ClearErrors
  ${If} $SessionDir == ""
    StrCpy $SessionErrorCode 87
    SetErrors
    Return
  ${EndIf}
  SetOutPath "$SessionDir"
  File /oname=MediaDrop.msi "${MSI_PATH}"
  File /oname=mediadrop-installer-worker.exe "${WORKER_PATH}"
  IfFileExists "$SessionDir\MediaDrop.msi" 0 ExtractMsiFailed
  IfFileExists "$SessionDir\mediadrop-installer-worker.exe" ExtractMsiDone 0
ExtractMsiFailed:
  StrCpy $SessionErrorCode 2
  SetErrors
  Return
ExtractMsiDone:
  ClearErrors
FunctionEnd

Function RegisterFonts
  System::Call 'gdi32::AddFontResourceExW(w "$PLUGINSDIR\ui\InstrumentSans-Regular.ttf", i 0x10, p 0) i .r0'
  System::Call 'gdi32::AddFontResourceExW(w "$PLUGINSDIR\ui\InstrumentSans-Medium.ttf", i 0x10, p 0) i .r0'
  System::Call 'gdi32::AddFontResourceExW(w "$PLUGINSDIR\ui\InstrumentSans-SemiBold.ttf", i 0x10, p 0) i .r0'
  System::Call 'gdi32::AddFontResourceExW(w "$PLUGINSDIR\ui\InstrumentSans-Bold.ttf", i 0x10, p 0) i .r0'
  CreateFont $FontNormal "Instrument Sans" 11 400
  CreateFont $FontSemibold "Instrument Sans" 11 600
  CreateFont $FontSmall "Instrument Sans" 9 600
  CreateFont $FontPercent "Instrument Sans" 40 700
FunctionEnd

Function ConfigureWindow
  System::Call 'user32::SetWindowTextW(p $HWNDPARENT, w "MediaDrop Kurulum") i .r0'
  System::Call 'user32::SetWindowLongW(p $HWNDPARENT, i ${GWL_STYLE}, i 0x96000000) i .r0'
  System::Call 'user32::GetSystemMetrics(i 0) i .r0'
  System::Call 'user32::GetSystemMetrics(i 1) i .r1'
  IntOp $2 $0 - 1120
  IntOp $2 $2 / 2
  IntOp $3 $1 - 650
  IntOp $3 $3 / 2
  ${If} $2 < 0
    StrCpy $2 0
  ${EndIf}
  ${If} $3 < 0
    StrCpy $3 0
  ${EndIf}
  System::Call 'user32::SetWindowPos(p $HWNDPARENT, p 0, i $2, i $3, i 1120, i 650, i 0x20) i .r0'
!ifdef UI_TEST_MODE
  ${If} $PreviewOffscreen == 1
    System::Call 'user32::GetWindowLongW(p $HWNDPARENT, i -20) i .r4'
    IntOp $4 $4 | 0x08000080
    System::Call 'user32::SetWindowLongW(p $HWNDPARENT, i -20, i $4) i .r0'
    System::Call 'user32::SetWindowPos(p $HWNDPARENT, p 0, i -32000, i -32000, i 1120, i 650, i 0x10) i .r0'
  ${EndIf}
!endif
  System::Call 'gdi32::CreateRoundRectRgn(i 0, i 0, i 1121, i 651, i 24, i 24) p .r0'
  System::Call 'user32::SetWindowRgn(p $HWNDPARENT, p $0, i 1) i .r1'
FunctionEnd

Function SetBackground
  Exch $0
  ${NSD_FreeBitmap} $BackgroundImage
  ${NSD_SetBitmap} $Background "$0" $BackgroundImage
  System::Call 'user32::SetWindowPos(p $Background, p 1, i 0, i 0, i 0, i 0, i 0x13) i .r1'
  Pop $0
FunctionEnd

Function RefreshDynamicUi
  ; Repaint children immediately without erasing the branded background first.
  System::Call 'user32::RedrawWindow(p $Dialog, p 0, p 0, i 0x1A1) i .r0'
FunctionEnd

Function HideNativeControls
  GetDlgItem $0 $HWNDPARENT 1
  ShowWindow $0 ${SW_HIDE}
  EnableWindow $0 0
  GetDlgItem $0 $HWNDPARENT 2
  ShowWindow $0 ${SW_HIDE}
  EnableWindow $0 0
  GetDlgItem $0 $HWNDPARENT 3
  ShowWindow $0 ${SW_HIDE}
  EnableWindow $0 0
  GetDlgItem $0 $HWNDPARENT 1028
  ShowWindow $0 ${SW_HIDE}
FunctionEnd

Function HideScreenControls
  ShowWindow $HoverVisual ${SW_HIDE}
  StrCpy $HoverTarget ""
  StrCpy $HoverProgress 0
  !insertmacro HideControl $StartHit
  !insertmacro HideControl $CancelHit
  !insertmacro HideControl $SummaryHit
  !insertmacro HideControl $FinishHit
  !insertmacro HideControl $LogHit
  !insertmacro HideControl $GiveUpHit
  !insertmacro HideControl $RetryHit
  !insertmacro HideControl $FilesRetryHit
  !insertmacro HideControl $FilesContinueHit
  !insertmacro HideControl $FilesCancelHit
  !insertmacro HideControl $WelcomeToggleHit
  !insertmacro HideControl $LaunchToggleHit
  !insertmacro HideControl $ExtensionToggleHit
  !insertmacro HideControl $WelcomeToggle
  !insertmacro HideControl $LaunchToggle
  !insertmacro HideControl $ExtensionToggle
  !insertmacro HideControl $ExtensionLaterHit
  !insertmacro HideControl $ExtensionCopyHit
  !insertmacro HideControl $ExtensionRevealHit
  !insertmacro HideControl $ExtensionPrimaryHit
  !insertmacro HideControl $ExtensionBrowserHit0
  !insertmacro HideControl $ExtensionBrowserHit1
  !insertmacro HideControl $ExtensionBrowserHit2
  !insertmacro HideControl $ExtensionBrowserHit3
  !insertmacro HideControl $ExtensionBrowserMask0
  !insertmacro HideControl $ExtensionBrowserMask1
  !insertmacro HideControl $ExtensionBrowserMask2
  !insertmacro HideControl $ExtensionBrowserMask3
  !insertmacro HideControl $ExtensionStep0
  !insertmacro HideControl $ExtensionStep1
  !insertmacro HideControl $ExtensionStep2
  !insertmacro HideControl $ExtensionStatusDot
  !insertmacro HideControl $ExtensionStatusTitle
  !insertmacro HideControl $ExtensionStatusDetail
  !insertmacro HideControl $Logo
  !insertmacro HideControl $ProgressFill
  !insertmacro HideControl $ProgressNumber
  !insertmacro HideControl $ProgressCurrent
  !insertmacro HideControl $StageDot0
  !insertmacro HideControl $StageDot1
  !insertmacro HideControl $StageDot2
  !insertmacro HideControl $StageDot3
  !insertmacro HideControl $StageName0
  !insertmacro HideControl $StageName1
  !insertmacro HideControl $StageName2
  !insertmacro HideControl $StageName3
  !insertmacro HideControl $StageState0
  !insertmacro HideControl $StageState1
  !insertmacro HideControl $StageState2
  !insertmacro HideControl $StageState3
  !insertmacro HideControl $ErrorLead
  !insertmacro HideControl $ErrorTitle
  !insertmacro HideControl $ErrorDetail
FunctionEnd

Function ShowWelcome
  !insertmacro RecordPreviewAction "screen:welcome"
  StrCpy $CurrentScreen "welcome"
  Call HideScreenControls
  Push "$PLUGINSDIR\ui\screen-welcome.bmp"
  Call SetBackground
  !insertmacro ShowControl $WelcomeToggle
  !insertmacro ShowControl $StartHit
  !insertmacro ShowControl $WelcomeToggleHit
  !insertmacro PrepareOverlay $StartHit
  !insertmacro PrepareOverlay $WelcomeToggleHit
FunctionEnd

Function ShowInstalling
  !insertmacro RecordPreviewAction "screen:installing"
  StrCpy $CurrentScreen "installing"
  Call HideScreenControls
  Push "$PLUGINSDIR\ui\screen-installing.bmp"
  Call SetBackground
  !insertmacro ShowControl $Logo
  !insertmacro ShowControl $ProgressFill
  !insertmacro ShowControl $ProgressNumber
  !insertmacro ShowControl $ProgressCurrent
  !insertmacro ShowControl $StageDot0
  !insertmacro ShowControl $StageDot1
  !insertmacro ShowControl $StageDot2
  !insertmacro ShowControl $StageDot3
  !insertmacro ShowControl $StageName0
  !insertmacro ShowControl $StageName1
  !insertmacro ShowControl $StageName2
  !insertmacro ShowControl $StageName3
  !insertmacro ShowControl $StageState0
  !insertmacro ShowControl $StageState1
  !insertmacro ShowControl $StageState2
  !insertmacro ShowControl $StageState3
  !insertmacro ShowControl $CancelHit
  !insertmacro PrepareOverlay $CancelHit
FunctionEnd

Function ResetDetectedBrowsers
  StrCpy $BrowserCount 0
  StrCpy $SelectedBrowserSlot -1
  StrCpy $BrowserId0 ""
  StrCpy $BrowserLabel0 ""
  StrCpy $BrowserExe0 ""
  StrCpy $BrowserPage0 ""
  StrCpy $BrowserId1 ""
  StrCpy $BrowserLabel1 ""
  StrCpy $BrowserExe1 ""
  StrCpy $BrowserPage1 ""
  StrCpy $BrowserId2 ""
  StrCpy $BrowserLabel2 ""
  StrCpy $BrowserExe2 ""
  StrCpy $BrowserPage2 ""
  StrCpy $BrowserId3 ""
  StrCpy $BrowserLabel3 ""
  StrCpy $BrowserExe3 ""
  StrCpy $BrowserPage3 ""
FunctionEnd

!ifdef UI_TEST_MODE
Function SeedTestBrowsers
  Call ResetDetectedBrowsers
  StrCpy $DefaultBrowserId "opera_gx"
  StrCpy $BrowserCount 3
  StrCpy $BrowserId0 "opera_gx"
  StrCpy $BrowserLabel0 "Opera GX ★"
  StrCpy $BrowserExe0 "C:\Program Files\Opera GX\opera.exe"
  StrCpy $BrowserPage0 "opera:extensions"
  StrCpy $BrowserId1 "chrome"
  StrCpy $BrowserLabel1 "Chrome"
  StrCpy $BrowserExe1 "C:\Program Files\Google\Chrome\Application\chrome.exe"
  StrCpy $BrowserPage1 "chrome://extensions"
  StrCpy $BrowserId2 "edge"
  StrCpy $BrowserLabel2 "Edge"
  StrCpy $BrowserExe2 "C:\Program Files\Microsoft\Edge\Application\msedge.exe"
  StrCpy $BrowserPage2 "edge://extensions"
FunctionEnd
!endif

!ifndef UI_TEST_MODE
Function AddDetectedBrowser
  ${If} $2 == ""
  ${OrIf} $BrowserCount >= 4
    Return
  ${EndIf}
  ${If} $BrowserCount == 0
    StrCpy $BrowserId0 $0
    StrCpy $BrowserLabel0 $1
    StrCpy $BrowserExe0 $2
    StrCpy $BrowserPage0 $3
  ${ElseIf} $BrowserCount == 1
    StrCpy $BrowserId1 $0
    StrCpy $BrowserLabel1 $1
    StrCpy $BrowserExe1 $2
    StrCpy $BrowserPage1 $3
  ${ElseIf} $BrowserCount == 2
    StrCpy $BrowserId2 $0
    StrCpy $BrowserLabel2 $1
    StrCpy $BrowserExe2 $2
    StrCpy $BrowserPage2 $3
  ${Else}
    StrCpy $BrowserId3 $0
    StrCpy $BrowserLabel3 $1
    StrCpy $BrowserExe3 $2
    StrCpy $BrowserPage3 $3
  ${EndIf}
  IntOp $BrowserCount $BrowserCount + 1
FunctionEnd

Function AddDefaultBrowserFirst
  ${If} $DefaultBrowserId == "opera_gx"
    StrCpy $0 "opera_gx"
    StrCpy $1 "Opera GX ★"
    StrCpy $2 $OperaGxExe
    StrCpy $3 "opera:extensions"
    Call AddDetectedBrowser
  ${ElseIf} $DefaultBrowserId == "opera"
    StrCpy $0 "opera"
    StrCpy $1 "Opera ★"
    StrCpy $2 $OperaExe
    StrCpy $3 "opera:extensions"
    Call AddDetectedBrowser
  ${ElseIf} $DefaultBrowserId == "chrome"
    StrCpy $0 "chrome"
    StrCpy $1 "Chrome ★"
    StrCpy $2 $ChromeExe
    StrCpy $3 "chrome://extensions"
    Call AddDetectedBrowser
  ${ElseIf} $DefaultBrowserId == "edge"
    StrCpy $0 "edge"
    StrCpy $1 "Edge ★"
    StrCpy $2 $EdgeExe
    StrCpy $3 "edge://extensions"
    Call AddDetectedBrowser
  ${EndIf}
FunctionEnd

Function DetectExtensionBrowsers
  Call ResetDetectedBrowsers
  StrCpy $OperaGxExe ""
  StrCpy $OperaExe ""
  StrCpy $ChromeExe ""
  StrCpy $EdgeExe ""

  IfFileExists "$LOCALAPPDATA\Programs\Opera GX\opera.exe" 0 opera_gx_fallback
    StrCpy $OperaGxExe "$LOCALAPPDATA\Programs\Opera GX\opera.exe"
    Goto opera_gx_ready
  opera_gx_fallback:
  IfFileExists "$LOCALAPPDATA\Programs\Opera GX\launcher.exe" 0 opera_gx_program_files
    StrCpy $OperaGxExe "$LOCALAPPDATA\Programs\Opera GX\launcher.exe"
    Goto opera_gx_ready
  opera_gx_program_files:
  IfFileExists "$PROGRAMFILES64\Opera GX\launcher.exe" 0 opera_gx_program_files_x86
    StrCpy $OperaGxExe "$PROGRAMFILES64\Opera GX\launcher.exe"
    Goto opera_gx_ready
  opera_gx_program_files_x86:
  IfFileExists "$PROGRAMFILES32\Opera GX\launcher.exe" 0 opera_gx_ready
    StrCpy $OperaGxExe "$PROGRAMFILES32\Opera GX\launcher.exe"
  opera_gx_ready:

  IfFileExists "$LOCALAPPDATA\Programs\Opera\opera.exe" 0 opera_fallback
    StrCpy $OperaExe "$LOCALAPPDATA\Programs\Opera\opera.exe"
    Goto opera_ready
  opera_fallback:
  IfFileExists "$LOCALAPPDATA\Programs\Opera\launcher.exe" 0 opera_program_files
    StrCpy $OperaExe "$LOCALAPPDATA\Programs\Opera\launcher.exe"
    Goto opera_ready
  opera_program_files:
  IfFileExists "$PROGRAMFILES64\Opera\launcher.exe" 0 opera_program_files_x86
    StrCpy $OperaExe "$PROGRAMFILES64\Opera\launcher.exe"
    Goto opera_ready
  opera_program_files_x86:
  IfFileExists "$PROGRAMFILES32\Opera\launcher.exe" 0 opera_ready
    StrCpy $OperaExe "$PROGRAMFILES32\Opera\launcher.exe"
  opera_ready:

  IfFileExists "$PROGRAMFILES64\Google\Chrome\Application\chrome.exe" 0 chrome_x86
    StrCpy $ChromeExe "$PROGRAMFILES64\Google\Chrome\Application\chrome.exe"
    Goto chrome_ready
  chrome_x86:
  IfFileExists "$PROGRAMFILES32\Google\Chrome\Application\chrome.exe" 0 chrome_local
    StrCpy $ChromeExe "$PROGRAMFILES32\Google\Chrome\Application\chrome.exe"
    Goto chrome_ready
  chrome_local:
  IfFileExists "$LOCALAPPDATA\Google\Chrome\Application\chrome.exe" 0 chrome_app_path
    StrCpy $ChromeExe "$LOCALAPPDATA\Google\Chrome\Application\chrome.exe"
    Goto chrome_ready
  chrome_app_path:
  ReadRegStr $ChromeExe HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe" ""
  IfFileExists "$ChromeExe" chrome_ready 0
  SetRegView 64
  ReadRegStr $ChromeExe HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe" ""
  IfFileExists "$ChromeExe" chrome_ready 0
  SetRegView 32
  ReadRegStr $ChromeExe HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe" ""
  IfFileExists "$ChromeExe" chrome_ready 0
  StrCpy $ChromeExe ""
  chrome_ready:

  IfFileExists "$PROGRAMFILES64\Microsoft\Edge\Application\msedge.exe" 0 edge_x86
    StrCpy $EdgeExe "$PROGRAMFILES64\Microsoft\Edge\Application\msedge.exe"
    Goto edge_ready
  edge_x86:
  IfFileExists "$PROGRAMFILES32\Microsoft\Edge\Application\msedge.exe" 0 edge_local
    StrCpy $EdgeExe "$PROGRAMFILES32\Microsoft\Edge\Application\msedge.exe"
    Goto edge_ready
  edge_local:
  IfFileExists "$LOCALAPPDATA\Microsoft\Edge\Application\msedge.exe" 0 edge_app_path
    StrCpy $EdgeExe "$LOCALAPPDATA\Microsoft\Edge\Application\msedge.exe"
    Goto edge_ready
  edge_app_path:
  ReadRegStr $EdgeExe HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe" ""
  IfFileExists "$EdgeExe" edge_ready 0
  SetRegView 64
  ReadRegStr $EdgeExe HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe" ""
  IfFileExists "$EdgeExe" edge_ready 0
  SetRegView 32
  ReadRegStr $EdgeExe HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe" ""
  IfFileExists "$EdgeExe" edge_ready 0
  StrCpy $EdgeExe ""
  edge_ready:
  SetRegView 64

  ReadRegStr $DefaultProgId HKCU "Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice" "ProgId"
  StrCpy $DefaultBrowserId ""
  ${If} $DefaultProgId == "OperaGXStable"
  ${OrIf} $DefaultProgId == "Opera GXStable"
    StrCpy $DefaultBrowserId "opera_gx"
  ${ElseIf} $DefaultProgId == "OperaStable"
    StrCpy $DefaultBrowserId "opera"
  ${ElseIf} $DefaultProgId == "ChromeHTML"
    StrCpy $DefaultBrowserId "chrome"
  ${ElseIf} $DefaultProgId == "MSEdgeHTM"
    StrCpy $DefaultBrowserId "edge"
  ${EndIf}

  Call AddDefaultBrowserFirst
  ${If} $DefaultBrowserId != "opera_gx"
    StrCpy $0 "opera_gx"
    StrCpy $1 "Opera GX"
    StrCpy $2 $OperaGxExe
    StrCpy $3 "opera:extensions"
    Call AddDetectedBrowser
  ${EndIf}
  ${If} $DefaultBrowserId != "opera"
    StrCpy $0 "opera"
    StrCpy $1 "Opera"
    StrCpy $2 $OperaExe
    StrCpy $3 "opera:extensions"
    Call AddDetectedBrowser
  ${EndIf}
  ${If} $DefaultBrowserId != "chrome"
    StrCpy $0 "chrome"
    StrCpy $1 "Chrome"
    StrCpy $2 $ChromeExe
    StrCpy $3 "chrome://extensions"
    Call AddDetectedBrowser
  ${EndIf}
  ${If} $DefaultBrowserId != "edge"
    StrCpy $0 "edge"
    StrCpy $1 "Edge"
    StrCpy $2 $EdgeExe
    StrCpy $3 "edge://extensions"
    Call AddDetectedBrowser
  ${EndIf}
FunctionEnd
!endif

Function LoadSelectedBrowser
  ${If} $SelectedBrowserSlot == 0
    StrCpy $SelectedBrowserId $BrowserId0
    StrCpy $SelectedBrowserLabel $BrowserLabel0
    StrCpy $SelectedBrowserExe $BrowserExe0
    StrCpy $SelectedBrowserPage $BrowserPage0
  ${ElseIf} $SelectedBrowserSlot == 1
    StrCpy $SelectedBrowserId $BrowserId1
    StrCpy $SelectedBrowserLabel $BrowserLabel1
    StrCpy $SelectedBrowserExe $BrowserExe1
    StrCpy $SelectedBrowserPage $BrowserPage1
  ${ElseIf} $SelectedBrowserSlot == 2
    StrCpy $SelectedBrowserId $BrowserId2
    StrCpy $SelectedBrowserLabel $BrowserLabel2
    StrCpy $SelectedBrowserExe $BrowserExe2
    StrCpy $SelectedBrowserPage $BrowserPage2
  ${ElseIf} $SelectedBrowserSlot == 3
    StrCpy $SelectedBrowserId $BrowserId3
    StrCpy $SelectedBrowserLabel $BrowserLabel3
    StrCpy $SelectedBrowserExe $BrowserExe3
    StrCpy $SelectedBrowserPage $BrowserPage3
  ${Else}
    StrCpy $SelectedBrowserId ""
    StrCpy $SelectedBrowserLabel ""
    StrCpy $SelectedBrowserExe ""
    StrCpy $SelectedBrowserPage ""
  ${EndIf}
FunctionEnd

Function UpdateExtensionBrowserButtons
  !insertmacro HideControl $ExtensionBrowserHit0
  !insertmacro HideControl $ExtensionBrowserHit1
  !insertmacro HideControl $ExtensionBrowserHit2
  !insertmacro HideControl $ExtensionBrowserHit3
  !insertmacro ShowControl $ExtensionBrowserMask0
  !insertmacro ShowControl $ExtensionBrowserMask1
  !insertmacro ShowControl $ExtensionBrowserMask2
  !insertmacro ShowControl $ExtensionBrowserMask3
  ${If} $BrowserCount > 0
    ${NSD_SetText} $ExtensionBrowserHit0 $BrowserLabel0
    !insertmacro HideControl $ExtensionBrowserMask0
    !insertmacro ShowControl $ExtensionBrowserHit0
  ${EndIf}
  ${If} $BrowserCount > 1
    ${NSD_SetText} $ExtensionBrowserHit1 $BrowserLabel1
    !insertmacro HideControl $ExtensionBrowserMask1
    !insertmacro ShowControl $ExtensionBrowserHit1
  ${EndIf}
  ${If} $BrowserCount > 2
    ${NSD_SetText} $ExtensionBrowserHit2 $BrowserLabel2
    !insertmacro HideControl $ExtensionBrowserMask2
    !insertmacro ShowControl $ExtensionBrowserHit2
  ${EndIf}
  ${If} $BrowserCount > 3
    ${NSD_SetText} $ExtensionBrowserHit3 $BrowserLabel3
    !insertmacro HideControl $ExtensionBrowserMask3
    !insertmacro ShowControl $ExtensionBrowserHit3
  ${EndIf}

  SetCtlColors $ExtensionBrowserHit0 0xCCCBC5 transparent
  SetCtlColors $ExtensionBrowserHit1 0xCCCBC5 transparent
  SetCtlColors $ExtensionBrowserHit2 0xCCCBC5 transparent
  SetCtlColors $ExtensionBrowserHit3 0xCCCBC5 transparent
  ${If} $SelectedBrowserSlot == 0
    SetCtlColors $ExtensionBrowserHit0 0xF5C75D transparent
  ${ElseIf} $SelectedBrowserSlot == 1
    SetCtlColors $ExtensionBrowserHit1 0xF5C75D transparent
  ${ElseIf} $SelectedBrowserSlot == 2
    SetCtlColors $ExtensionBrowserHit2 0xF5C75D transparent
  ${ElseIf} $SelectedBrowserSlot == 3
    SetCtlColors $ExtensionBrowserHit3 0xF5C75D transparent
  ${EndIf}
  Call RefreshDynamicUi
FunctionEnd

Function UpdateExtensionGuide
  Call LoadSelectedBrowser
  Call UpdateExtensionBrowserButtons
  ${If} $SelectedBrowserId == ""
    ${NSD_SetText} $ExtensionStep0 "Opera, Opera GX, Chrome veya Edge kurulumu bulunamadı."
    ${NSD_SetText} $ExtensionStep1 "Desteklenen bir tarayıcı kurduktan sonra bu adımı tekrar açabilirsin."
    ${NSD_SetText} $ExtensionStep2 "MediaDrop kurulumu eklenti olmadan da tamamlanabilir."
    ${NSD_SetText} $ExtensionStatusTitle "Desteklenen tarayıcı bulunamadı"
    ${NSD_SetText} $ExtensionStatusDetail "Daha sonra uygulamadaki Eklentiyi kur veya onar seçeneğini kullanabilirsin."
    ${NSD_SetText} $ExtensionPrimaryHit "Tarayıcı bulunamadı"
    SetCtlColors $ExtensionStatusDot 0xFF8C9C 0x202126
    SetCtlColors $ExtensionPrimaryHit 0x8F8F8B 0x25262B
    EnableWindow $ExtensionPrimaryHit 0
    EnableWindow $ExtensionCopyHit 0
    EnableWindow $ExtensionRevealHit 0
    Call RefreshDynamicUi
    Return
  ${EndIf}

  ${If} $ExtensionPath == ""
    ${NSD_SetText} $ExtensionStep0 "MediaDrop kuruldu ancak eklenti kaynak klasörü bulunamadı."
    ${NSD_SetText} $ExtensionStep1 "Kurulumu Onar seçeneği kaynak dosyalarını yeniden yerleştirebilir."
    ${NSD_SetText} $ExtensionStep2 "Şimdilik Daha sonra ile MediaDrop kurulumunu tamamlayabilirsin."
    ${NSD_SetText} $ExtensionStatusTitle "Eklenti dosyaları eksik"
    ${NSD_SetText} $ExtensionStatusDetail "Kurulum günlüğü ve MSI Onar işlemiyle yeniden deneyebilirsin."
    ${NSD_SetText} $ExtensionPrimaryHit "Eklenti bulunamadı"
    SetCtlColors $ExtensionStatusDot 0xFF8C9C 0x202126
    SetCtlColors $ExtensionPrimaryHit 0x8F8F8B 0x25262B
    EnableWindow $ExtensionPrimaryHit 0
    EnableWindow $ExtensionCopyHit 0
    EnableWindow $ExtensionRevealHit 0
    Call RefreshDynamicUi
    Return
  ${EndIf}

  EnableWindow $ExtensionPrimaryHit 1
  EnableWindow $ExtensionCopyHit 1
  EnableWindow $ExtensionRevealHit 1
  SetCtlColors $ExtensionPrimaryHit 0x171512 transparent
  SetCtlColors $ExtensionCopyHit 0xCCCBC5 transparent
  SetCtlColors $ExtensionRevealHit 0xCCCBC5 transparent
  ${If} $ExtensionConnected == 1
    ${NSD_SetText} $ExtensionStep0 "✓ $SelectedBrowserLabel tarayıcısı hazır."
    ${NSD_SetText} $ExtensionStep1 "✓ MediaDrop eklentisi tarayıcıda çalışıyor."
    ${NSD_SetText} $ExtensionStep2 "✓ Güvenli yerel bağlantı doğrulandı."
  ${ElseIf} $ExtensionBrowserOpened == 1
    ${NSD_SetText} $ExtensionStep0 "✓ $SelectedBrowserLabel açma komutu gönderildi."
    ${If} $SelectedBrowserId == "opera_gx"
    ${OrIf} $SelectedBrowserId == "opera"
      ${NSD_SetText} $ExtensionStep1 "opera:extensions panoda. Adres çubuğuna yapıştır veya Ctrl+Shift+E kullan."
    ${Else}
      ${NSD_SetText} $ExtensionStep1 "Sayfa görünmediyse $SelectedBrowserPage adresini elle gir."
    ${EndIf}
    ${If} $ExtensionPathCopied == 1
      ${NSD_SetText} $ExtensionStep2 "✓ Eklenti yolu panoda. Klasörü göster seçeneği de hazır."
    ${Else}
      ${NSD_SetText} $ExtensionStep2 "Geliştirici modunu aç; Yolu kopyala veya Klasörü göster ile dizini seç."
    ${EndIf}
  ${Else}
    ${If} $SelectedBrowserId == "opera_gx"
    ${OrIf} $SelectedBrowserId == "opera"
      ${NSD_SetText} $ExtensionStep0 "$SelectedBrowserLabel tarayıcısını aç."
      ${NSD_SetText} $ExtensionStep1 "opera:extensions adresini yapıştır veya Ctrl+Shift+E kullan."
    ${Else}
      ${NSD_SetText} $ExtensionStep0 "$SelectedBrowserLabel tarayıcısını ve eklenti sayfasını açmayı dene."
      ${NSD_SetText} $ExtensionStep1 "Gerekirse $SelectedBrowserPage adresini elle gir."
    ${EndIf}
    ${NSD_SetText} $ExtensionStep2 "Geliştirici modunu aç; Yolu kopyala veya Klasörü göster ile dizini seç."
  ${EndIf}

  ${If} $ExtensionConnected == 1
    ${NSD_SetText} $ExtensionStatusTitle "Eklenti bağlı ✓"
    ${NSD_SetText} $ExtensionStatusDetail "$SelectedBrowserLabel ile MediaDrop arasındaki güvenli köprü hazır."
    ${NSD_SetText} $ExtensionPrimaryHit "Devam"
    SetCtlColors $ExtensionStatusDot 0x78E5AF 0x202126
    SetCtlColors $ExtensionStatusTitle 0x78E5AF 0x202126
  ${ElseIf} $ExtensionBrowserOpened == 1
    ${NSD_SetText} $ExtensionStatusTitle "Tarayıcı adımı açık"
    ${NSD_SetText} $ExtensionStatusDetail "Sayfa dışarıdan doğrulanmaz; eklenti bağlanınca otomatik tamamlanır."
    ${NSD_SetText} $ExtensionPrimaryHit "Eklentiyi yükledim"
    SetCtlColors $ExtensionStatusDot 0xF5C75D 0x202126
    SetCtlColors $ExtensionStatusTitle 0xF5C75D 0x202126
  ${Else}
    ${NSD_SetText} $ExtensionStatusTitle "Tarayıcı adımı hazır"
    ${NSD_SetText} $ExtensionStatusDetail "Tarayıcıyı aç; gösterilen adres ve kısayol adımlarını izle."
    ${If} $SelectedBrowserId == "opera_gx"
    ${OrIf} $SelectedBrowserId == "opera"
      ${NSD_SetText} $ExtensionPrimaryHit "Tarayıcıyı aç"
    ${Else}
      ${NSD_SetText} $ExtensionPrimaryHit "Sayfayı açmayı dene"
    ${EndIf}
    SetCtlColors $ExtensionStatusDot 0xF5C75D 0x202126
    SetCtlColors $ExtensionStatusTitle 0xF6F5F1 0x202126
  ${EndIf}
  Call RefreshDynamicUi
FunctionEnd

!ifndef UI_TEST_MODE
Function EnsureExtensionConnectionEvent
  ${If} $ExtensionEvent != 0
    System::Call 'kernel32::CloseHandle(p $ExtensionEvent) i .r0'
    StrCpy $ExtensionEvent 0
  ${EndIf}
  System::Call 'kernel32::CreateEventW(p 0, i 1, i 0, w "${EXTENSION_CONNECTED_EVENT}") p .r0'
  StrCpy $ExtensionEvent $0
  ${If} $ExtensionEvent != 0
    System::Call 'kernel32::ResetEvent(p $ExtensionEvent) i .r0'
  ${EndIf}
FunctionEnd
!endif

Function CopyClipboardText
  StrCpy $0 0
  ${If} $4 == ""
    Return
  ${EndIf}
!ifdef UI_TEST_MODE
  !insertmacro RecordPreviewAction "clipboard:$4"
  StrCpy $0 1
  Return
!endif
  System::Call 'user32::OpenClipboard(p $HWNDPARENT) i .r0'
  ${If} $0 == 0
    Return
  ${EndIf}
  System::Call 'user32::EmptyClipboard() i .r0'
  StrLen $1 $4
  IntOp $1 $1 + 1
  IntOp $1 $1 * 2
  System::Call 'kernel32::GlobalAlloc(i 0x42, i $1) p .r2'
  ${If} $2 == 0
    System::Call 'user32::CloseClipboard() i .r0'
    Return
  ${EndIf}
  System::Call 'kernel32::GlobalLock(p $2) p .r3'
  ${If} $3 == 0
    System::Call 'kernel32::GlobalFree(p $2) p .r0'
    System::Call 'user32::CloseClipboard() i .r0'
    Return
  ${EndIf}
  System::Call 'kernel32::lstrcpyW(p $3, w "$4") p .r0'
  System::Call 'kernel32::GlobalUnlock(p $2) i .r0'
  System::Call 'user32::SetClipboardData(i 13, p $2) p .r0'
  ${If} $0 == 0
    System::Call 'kernel32::GlobalFree(p $2) p .r0'
    System::Call 'user32::CloseClipboard() i .r0'
    Return
  ${EndIf}
  System::Call 'user32::CloseClipboard() i .r0'
  StrCpy $0 1
FunctionEnd

Function CopyExtensionPath
  StrCpy $ExtensionPathCopied 0
  StrCpy $4 $ExtensionPath
  Call CopyClipboardText
  StrCpy $ExtensionPathCopied $0
FunctionEnd

Function CopyExtensionPage
  StrCpy $4 $SelectedBrowserPage
  Call CopyClipboardText
FunctionEnd

Function PrepareExtensionSetup
  StrCpy $ExtensionConnected 0
  StrCpy $ExtensionBrowserOpened 0
  StrCpy $ExtensionPathCopied 0
!ifdef UI_TEST_MODE
  Call SeedTestBrowsers
!else
  Call DetectExtensionBrowsers
!endif
  ${If} $BrowserCount > 0
    StrCpy $SelectedBrowserSlot 0
  ${EndIf}
!ifdef UI_TEST_MODE
  StrCpy $ExtensionPath "C:\Program Files\MediaDrop\browser-extension"
!else
  Call ResolveMediaDropExecutable
  StrCpy $MediaDropExe $0
  ${GetParent} "$MediaDropExe" $ExtensionRoot
  StrCpy $ExtensionPath "$ExtensionRoot\browser-extension"
  IfFileExists "$ExtensionPath\manifest.json" extension_path_ready 0
    StrCpy $ExtensionPath "$ExtensionRoot\resources\browser-extension"
  IfFileExists "$ExtensionPath\manifest.json" extension_path_ready 0
    StrCpy $ExtensionPath ""
  extension_path_ready:
  Call EnsureExtensionConnectionEvent
  IfFileExists "$MediaDropExe" 0 companion_started
    Exec '"$MediaDropExe" --companion'
  companion_started:
!endif
  Call UpdateExtensionGuide
FunctionEnd

Function ShowExtensionSetup
  !insertmacro RecordPreviewAction "screen:extension"
  ${NSD_KillTimer} PollInstallation
  StrCpy $CurrentScreen "extension"
  Call HideScreenControls
  Push "$PLUGINSDIR\ui\screen-extension.bmp"
  Call SetBackground
  !insertmacro ShowControl $ExtensionStep0
  !insertmacro ShowControl $ExtensionStep1
  !insertmacro ShowControl $ExtensionStep2
  !insertmacro ShowControl $ExtensionStatusDot
  !insertmacro ShowControl $ExtensionStatusTitle
  !insertmacro ShowControl $ExtensionStatusDetail
  Call PrepareExtensionSetup
  !insertmacro ShowControl $ExtensionLaterHit
  !insertmacro ShowControl $ExtensionCopyHit
  !insertmacro ShowControl $ExtensionRevealHit
  !insertmacro ShowControl $ExtensionPrimaryHit
  !insertmacro PrepareOverlay $ExtensionLaterHit
  !insertmacro PrepareOverlay $ExtensionCopyHit
  !insertmacro PrepareOverlay $ExtensionRevealHit
  !insertmacro PrepareOverlay $ExtensionPrimaryHit
  ${NSD_CreateTimer} PollExtensionConnection 700
FunctionEnd

Function PollExtensionConnection
  ${If} $CurrentScreen != "extension"
  ${OrIf} $ExtensionConnected == 1
  ${OrIf} $ExtensionEvent == 0
    Return
  ${EndIf}
  System::Call 'kernel32::WaitForSingleObject(p $ExtensionEvent, i 0) i .r0'
  ${If} $0 == 0
    StrCpy $ExtensionConnected 1
    ${NSD_KillTimer} PollExtensionConnection
    Call UpdateExtensionGuide
  ${EndIf}
FunctionEnd

Function SelectExtensionBrowser0
  Pop $0
  !insertmacro RecordPreviewAction "browser_0"
  StrCpy $SelectedBrowserSlot 0
  StrCpy $ExtensionBrowserOpened 0
  Call UpdateExtensionGuide
FunctionEnd

Function SelectExtensionBrowser1
  Pop $0
  !insertmacro RecordPreviewAction "browser_1"
  StrCpy $SelectedBrowserSlot 1
  StrCpy $ExtensionBrowserOpened 0
  Call UpdateExtensionGuide
FunctionEnd

Function SelectExtensionBrowser2
  Pop $0
  !insertmacro RecordPreviewAction "browser_2"
  StrCpy $SelectedBrowserSlot 2
  StrCpy $ExtensionBrowserOpened 0
  Call UpdateExtensionGuide
FunctionEnd

Function SelectExtensionBrowser3
  Pop $0
  !insertmacro RecordPreviewAction "browser_3"
  StrCpy $SelectedBrowserSlot 3
  StrCpy $ExtensionBrowserOpened 0
  Call UpdateExtensionGuide
FunctionEnd

Function OnExtensionCopyPath
  Pop $0
  !insertmacro RecordPreviewAction "copy_extension_path"
  Call CopyExtensionPath
  Call UpdateExtensionGuide
FunctionEnd

Function OnExtensionRevealPath
  Pop $0
  !insertmacro RecordPreviewAction "reveal_extension_path"
!ifndef UI_TEST_MODE
  IfFileExists "$ExtensionPath\manifest.json" 0 +2
    ExecShell "open" "explorer.exe" '/select,"$ExtensionPath\manifest.json"'
!endif
FunctionEnd

Function OnExtensionConfirm
  ${NSD_KillTimer} PollExtensionConnection
  StrCpy $ExtensionHandled 1
  Call ShowDone
FunctionEnd

Function OnExtensionPrimary
  Pop $0
  !insertmacro RecordPreviewAction "extension_primary"
  ${If} $ExtensionConnected == 1
  ${OrIf} $ExtensionBrowserOpened == 1
    Call OnExtensionConfirm
    Return
  ${EndIf}
  Call LoadSelectedBrowser
  ${If} $SelectedBrowserExe == ""
    Return
  ${EndIf}
!ifdef UI_TEST_MODE
  ${If} $SelectedBrowserId == "opera_gx"
  ${OrIf} $SelectedBrowserId == "opera"
    !insertmacro RecordPreviewAction "copy_extension_address:$SelectedBrowserPage"
    Call CopyExtensionPage
    !insertmacro RecordPreviewAction "browser_launch:$SelectedBrowserExe|-noautoupdate --|"
  ${Else}
    !insertmacro RecordPreviewAction "browser_launch:$SelectedBrowserExe||$SelectedBrowserPage"
  ${EndIf}
  StrCpy $ExtensionBrowserOpened 1
  Call UpdateExtensionGuide
  Return
!else
  ${If} $SelectedBrowserId == "opera_gx"
  ${OrIf} $SelectedBrowserId == "opera"
    Call CopyExtensionPage
    Exec '"$SelectedBrowserExe" -noautoupdate --'
  ${Else}
    Exec '"$SelectedBrowserExe" "$SelectedBrowserPage"'
  ${EndIf}
  StrCpy $ExtensionBrowserOpened 1
  Call UpdateExtensionGuide
!endif
FunctionEnd

Function OnExtensionLater
  Pop $0
  !insertmacro RecordPreviewAction "extension_later"
  ${NSD_KillTimer} PollExtensionConnection
  StrCpy $ExtensionSetup 0
  StrCpy $ExtensionHandled 1
  Call ShowDone
FunctionEnd

Function SyncDoneExtensionToggle
  ${NSD_FreeBitmap} $ExtensionToggleImage
  ${If} $ExtensionSetup == 1
    ${NSD_SetBitmap} $ExtensionToggle "$PLUGINSDIR\ui\toggle-6.bmp" $ExtensionToggleImage
  ${Else}
    ${NSD_SetBitmap} $ExtensionToggle "$PLUGINSDIR\ui\toggle-0.bmp" $ExtensionToggleImage
  ${EndIf}
FunctionEnd

Function ShowDone
  !insertmacro RecordPreviewAction "screen:done"
  ${NSD_KillTimer} PollInstallation
  ${NSD_KillTimer} PollExtensionConnection
  StrCpy $CurrentScreen "done"
  Call HideScreenControls
  Push "$PLUGINSDIR\ui\screen-done.bmp"
  Call SetBackground
  !insertmacro ShowControl $LaunchToggle
  Call SyncDoneExtensionToggle
  !insertmacro ShowControl $ExtensionToggle
  !insertmacro ShowControl $SummaryHit
  !insertmacro ShowControl $FinishHit
  !insertmacro ShowControl $LaunchToggleHit
  !insertmacro ShowControl $ExtensionToggleHit
  !insertmacro PrepareOverlay $SummaryHit
  !insertmacro PrepareOverlay $FinishHit
  !insertmacro PrepareOverlay $LaunchToggleHit
  !insertmacro PrepareOverlay $ExtensionToggleHit
FunctionEnd

Function EnsureInstallDiagnostic
!ifdef UI_TEST_MODE
  Return
!else
  ${If} $InstallFailureKind == "outer_elevated"
    Return
  ${EndIf}
  ${If} $InstallLog == ""
    CreateDirectory "$LOCALAPPDATA\MediaDrop\Kurulum Günlükleri"
    StrCpy $InstallLog "$LOCALAPPDATA\MediaDrop\Kurulum Günlükleri\MediaDrop-Setup-${APP_VERSION}.log"
  ${EndIf}
  IfFileExists "$InstallLog" EnsureInstallDiagnosticDone 0
  ClearErrors
  FileOpen $0 "$InstallLog" w
  ${IfNot} ${Errors}
    FileWrite $0 "MediaDrop ${APP_VERSION} kurulum tanısı$\r$\n"
    FileWrite $0 "Aşama: $InstallFailureKind$\r$\n"
    FileWrite $0 "Windows hata kodu: $InstallExitCode$\r$\n"
    FileClose $0
  ${EndIf}
EnsureInstallDiagnosticDone:
!endif
FunctionEnd

Function SetErrorCopy
  ${If} $InstallFailureKind == "outer_elevated"
    ${NSD_SetText} $ErrorLead "Kurucuyu yönetici olarak açtığında tarayıcı ve kullanıcı profili işlemleri yanlış hesaba gidebilir. Bu pencereyi kapatıp setup dosyasını normal biçimde aç."
    ${NSD_SetText} $ErrorTitle "Kurucuyu normal biçimde aç"
    ${NSD_SetText} $ErrorDetail "Dosyaya çift tıkla; yönetici iznini kurulum başladığında MediaDrop kendisi isteyecek."
  ${ElseIf} $InstallFailureKind == "elevation_cancelled"
    ${NSD_SetText} $ErrorLead "Windows yönetici izni verilmediği için MediaDrop kurulmadı. Hazır olduğunda tekrar deneyebilirsin."
    ${NSD_SetText} $ErrorTitle "Kurulum izni verilmedi"
    ${NSD_SetText} $ErrorDetail "Bilgisayarında hiçbir değişiklik yapılmadı. Windows hata kodu: 1223"
  ${ElseIf} $InstallFailureKind == "broker_launch"
  ${OrIf} $InstallFailureKind == "session"
    ${NSD_SetText} $ErrorLead "Güvenli kurulum hizmeti başlatılamadı. Günlüğü açıp ayrıntıyı görebilir veya tekrar deneyebilirsin."
    ${NSD_SetText} $ErrorTitle "Kurulum hizmeti başlatılamadı"
    ${NSD_SetText} $ErrorDetail "Windows hata kodu: $InstallExitCode"
  ${ElseIf} $InstallFailureKind == "status_timeout"
  ${OrIf} $InstallFailureKind == "status_invalid"
    ${NSD_SetText} $ErrorLead "Kurulum hizmetinden güvenilir bir sonuç alınamadı. Günlüğü açabilir veya tekrar deneyebilirsin."
    ${NSD_SetText} $ErrorTitle "Kurulum bağlantısı kesildi"
    ${NSD_SetText} $ErrorDetail "Windows hata kodu: $InstallExitCode"
  ${ElseIf} $InstallExitCode == 1602
    ${NSD_SetText} $ErrorLead "Kurulum kullanıcı isteğiyle güvenli biçimde durduruldu. İstersen aynı paketten yeniden başlayabilirsin."
    ${NSD_SetText} $ErrorTitle "Kurulum iptal edildi"
    ${NSD_SetText} $ErrorDetail "Windows Installer değişiklikleri geri aldı. Hata kodu: 1602"
  ${ElseIf} $InstallExitCode == 1618
    ${NSD_SetText} $ErrorLead "Windows üzerinde başka bir kurulum devam ediyor. O işlem tamamlandıktan sonra tekrar deneyebilirsin."
    ${NSD_SetText} $ErrorTitle "Başka bir kurulum çalışıyor"
    ${NSD_SetText} $ErrorDetail "Devam eden Windows Installer işlemi MediaDrop kurulumunu bekletiyor. Hata kodu: 1618"
  ${ElseIf} $InstallExitCode == 1603
    ${NSD_SetText} $ErrorLead "Windows Installer beklenmeyen bir hatayla durdu. Kesin nedeni görmek için kurulum günlüğünü açıp ayrıntıları inceleyebilirsin."
    ${NSD_SetText} $ErrorTitle "Kurulum tamamlanamadı"
    ${NSD_SetText} $ErrorDetail "Kurulum günlüğü ayrıntıları içerir. Windows Installer hata kodu: 1603"
  ${Else}
    ${NSD_SetText} $ErrorLead "Kurulum güvenli biçimde durdu. Günlüğü açabilir veya aynı paketten yeniden deneyebilirsin."
    ${NSD_SetText} $ErrorTitle "Windows Installer işlemi durdu"
    ${NSD_SetText} $ErrorDetail "Windows Installer hata kodu: $InstallExitCode"
  ${EndIf}
FunctionEnd

Function ShowError
  !insertmacro RecordPreviewAction "screen:error"
  ${NSD_KillTimer} PollInstallation
  Call EnsureInstallDiagnostic
  StrCpy $CurrentScreen "error"
  Call HideScreenControls
  Push "$PLUGINSDIR\ui\screen-error.bmp"
  Call SetBackground
  Call SetErrorCopy
  !insertmacro ShowControl $ErrorLead
  !insertmacro ShowControl $ErrorTitle
  !insertmacro ShowControl $ErrorDetail
  !insertmacro ShowControl $GiveUpHit
  !insertmacro PrepareOverlay $GiveUpHit
  ${If} $InstallFailureKind != "outer_elevated"
    !insertmacro ShowControl $LogHit
    !insertmacro PrepareOverlay $LogHit
    ${If} $BrokerStarted == 0
      !insertmacro ShowControl $RetryHit
      !insertmacro PrepareOverlay $RetryHit
    ${EndIf}
  ${EndIf}
FunctionEnd

Function UpdateStages
  StrCpy $0 0
  ${If} $Progress >= 84
    StrCpy $0 3
  ${ElseIf} $Progress >= 60
    StrCpy $0 2
  ${ElseIf} $Progress >= 28
    StrCpy $0 1
  ${EndIf}
  ${If} $Progress >= 100
    StrCpy $0 4
  ${EndIf}
  ${If} $0 == $LastStage
    Return
  ${EndIf}
  StrCpy $LastStage $0

  SetCtlColors $StageDot0 0x55565C 0x17181C
  SetCtlColors $StageDot1 0x55565C 0x17181C
  SetCtlColors $StageDot2 0x55565C 0x17181C
  SetCtlColors $StageDot3 0x55565C 0x17181C
  SetCtlColors $StageName0 0x72726F 0x17181C
  SetCtlColors $StageName1 0x72726F 0x17181C
  SetCtlColors $StageName2 0x72726F 0x17181C
  SetCtlColors $StageName3 0x72726F 0x17181C
  SetCtlColors $StageState0 0x72726F 0x17181C
  SetCtlColors $StageState1 0x72726F 0x17181C
  SetCtlColors $StageState2 0x72726F 0x17181C
  SetCtlColors $StageState3 0x72726F 0x17181C
  ${NSD_SetText} $StageState0 "BEKLİYOR"
  ${NSD_SetText} $StageState1 "BEKLİYOR"
  ${NSD_SetText} $StageState2 "BEKLİYOR"
  ${NSD_SetText} $StageState3 "BEKLİYOR"

  ${If} $0 > 0
    SetCtlColors $StageDot0 0xDFA326 0x17181C
    SetCtlColors $StageName0 0xCCCBC5 0x17181C
    SetCtlColors $StageState0 0xCCCBC5 0x17181C
    ${NSD_SetText} $StageState0 "TAMAM"
  ${EndIf}
  ${If} $0 > 1
    SetCtlColors $StageDot1 0xDFA326 0x17181C
    SetCtlColors $StageName1 0xCCCBC5 0x17181C
    SetCtlColors $StageState1 0xCCCBC5 0x17181C
    ${NSD_SetText} $StageState1 "TAMAM"
  ${EndIf}
  ${If} $0 > 2
    SetCtlColors $StageDot2 0xDFA326 0x17181C
    SetCtlColors $StageName2 0xCCCBC5 0x17181C
    SetCtlColors $StageState2 0xCCCBC5 0x17181C
    ${NSD_SetText} $StageState2 "TAMAM"
  ${EndIf}
  ${If} $0 > 3
    SetCtlColors $StageDot3 0xDFA326 0x17181C
    SetCtlColors $StageName3 0xCCCBC5 0x17181C
    SetCtlColors $StageState3 0xCCCBC5 0x17181C
    ${NSD_SetText} $StageState3 "TAMAM"
  ${EndIf}

  ${If} $0 == 0
    SetCtlColors $StageDot0 0xF5C75D 0x17181C
    SetCtlColors $StageName0 0xCCCBC5 0x17181C
    SetCtlColors $StageState0 0xCCCBC5 0x17181C
    ${NSD_SetText} $StageState0 "ŞİMDİ"
  ${ElseIf} $0 == 1
    SetCtlColors $StageDot1 0xF5C75D 0x17181C
    SetCtlColors $StageName1 0xCCCBC5 0x17181C
    SetCtlColors $StageState1 0xCCCBC5 0x17181C
    ${NSD_SetText} $StageState1 "ŞİMDİ"
  ${ElseIf} $0 == 2
    SetCtlColors $StageDot2 0xF5C75D 0x17181C
    SetCtlColors $StageName2 0xCCCBC5 0x17181C
    SetCtlColors $StageState2 0xCCCBC5 0x17181C
    ${NSD_SetText} $StageState2 "ŞİMDİ"
  ${ElseIf} $0 == 3
    SetCtlColors $StageDot3 0xF5C75D 0x17181C
    SetCtlColors $StageName3 0xCCCBC5 0x17181C
    SetCtlColors $StageState3 0xCCCBC5 0x17181C
    ${NSD_SetText} $StageState3 "ŞİMDİ"
  ${Else}
    ${NSD_SetText} $ProgressCurrent "Kurulum tamamlandı"
  ${EndIf}
FunctionEnd

Function UpdateProgress
  ${If} $Progress < 0
    StrCpy $Progress 0
  ${ElseIf} $Progress > 100
    StrCpy $Progress 100
  ${EndIf}
  ${NSD_SetText} $ProgressNumber "$Progress%"

  StrCpy $0 $Progress
  ${If} $0 < 2
    StrCpy $0 2
  ${EndIf}
  IntOp $1 582 * $0
  IntOp $1 $1 / 100
  ${If} $1 < 12
    StrCpy $1 12
  ${EndIf}
  System::Call 'user32::SetWindowPos(p $ProgressFill, p 0, i 482, i 300, i $1, i 10, i 0x14) i .r2'

  ${If} $Progress < 10
    StrCpy $0 "00$Progress"
  ${ElseIf} $Progress < 100
    StrCpy $0 "0$Progress"
  ${Else}
    StrCpy $0 "100"
  ${EndIf}
  ${NSD_FreeBitmap} $LogoImage
  ${NSD_SetBitmap} $Logo "$PLUGINSDIR\ui\logo\logo-$0.bmp" $LogoImage
  Call UpdateStages
  Call RefreshDynamicUi
FunctionEnd

Function CreateInstallerSession
  ClearErrors
  StrCpy $SessionErrorCode 0
  StrCpy $SessionRoot "$LOCALAPPDATA\MediaDrop\InstallerSessions"
  CreateDirectory "$LOCALAPPDATA\MediaDrop"
  CreateDirectory "$SessionRoot"
  System::Call 'ole32::CoCreateGuid(g .s)'
  Pop $0
  StrCpy $SessionId $0 36 1
  ${StrCase} $SessionId $SessionId "L"
  StrCpy $SessionDir "$SessionRoot\$SessionId"

  ClearErrors
  CreateDirectory "$SessionDir"
  ${If} ${Errors}
    System::Call 'kernel32::GetLastError() i .r0'
    StrCpy $SessionErrorCode $0
    SetErrors
    Return
  ${EndIf}

  StrCpy $StatusPath "$SessionDir\status.ini"
  StrCpy $CommandPath "$SessionDir\command.ini"
  System::Call 'kernel32::GetCurrentProcessId() i .r0'
  FileOpen $1 "$SessionDir\config.ini" w
  ${If} ${Errors}
    System::Call 'kernel32::GetLastError() i .r0'
    StrCpy $SessionErrorCode $0
    SetErrors
    Return
  ${EndIf}
  FileWriteUTF16LE /BOM $1 "[session]$\r$\nprotocol=1$\r$\nsession_id=$SessionId$\r$\nparent_pid=$0$\r$\nsilent=$SilentMode$\r$\n"
!ifdef UI_TEST_MODE
  FileWriteUTF16LE $1 "[test]$\r$\nscenario=$TestWorkerScenario$\r$\n"
!endif
  FileClose $1
  !insertmacro RecordPreviewAction "session:$SessionId"
FunctionEnd

Function CleanupInstallerSession
  ${If} $SessionDir == ""
    Return
  ${EndIf}
  Push $0
  SetOutPath "$PLUGINSDIR"
  StrCpy $0 0
CleanupInstallerSessionRetry:
  Delete "$SessionDir\command.tmp"
  Delete "$SessionDir\command.ini"
  Delete "$SessionDir\status.ini"
  Delete "$SessionDir\config.ini"
  Delete "$SessionDir\MediaDrop.msi"
  Delete "$SessionDir\mediadrop-installer-worker.exe"
  ClearErrors
  RMDir "$SessionDir"
  ${IfNot} ${Errors}
    Goto CleanupInstallerSessionDone
  ${EndIf}
  IntOp $0 $0 + 1
  ${If} $0 < 20
    Sleep 50
    Goto CleanupInstallerSessionRetry
  ${EndIf}
CleanupInstallerSessionDone:
  StrCpy $SessionDir ""
  StrCpy $StatusPath ""
  StrCpy $CommandPath ""
  Pop $0
FunctionEnd

Function SendBrokerCommand
  Exch $0
  Push $1
  Push $2
  Push $3
  Push $4
  ClearErrors
  ${If} $CommandPath == ""
    SetErrors
    Goto SendBrokerCommandDone
  ${EndIf}
  IntOp $CommandSequence $CommandSequence + 1
  StrCpy $1 "$SessionDir\command.tmp"
  Delete "$1"
  FileOpen $2 "$1" w
  ${If} ${Errors}
    Goto SendBrokerCommandDone
  ${EndIf}
  FileWriteUTF16LE /BOM $2 "[command]$\r$\nprotocol=1$\r$\nsequence=$CommandSequence$\r$\ncommand=$0$\r$\nresponse=$\r$\n"
  FileClose $2
  StrCpy $3 0
SendBrokerCommandReplace:
  System::Call 'kernel32::MoveFileExW(w "$1", w "$CommandPath", i 9) i .r4'
  ${If} $4 != 0
    ClearErrors
    Goto SendBrokerCommandDone
  ${EndIf}
  IntOp $3 $3 + 1
  ${If} $3 < 20
    Sleep 5
    Goto SendBrokerCommandReplace
  ${EndIf}
  SetErrors
SendBrokerCommandDone:
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

Function ReadInstallerStatus
  StrCpy $StatusChanged 0
  ClearErrors
  IfFileExists "$StatusPath" 0 ReadInstallerStatusMissing
  ReadINIStr $0 "$StatusPath" "status" "protocol"
  ${If} ${Errors}
  ${OrIf} $0 != 1
    SetErrors
    Return
  ${EndIf}
  ReadINIStr $0 "$StatusPath" "status" "sequence"
  ${If} ${Errors}
    Return
  ${EndIf}
  ${If} $0 <= $StatusSequence
    ClearErrors
    Return
  ${EndIf}
  StrCpy $StatusSequence $0
  StrCpy $StatusChanged 1
  StrCpy $InstallTick 0
  ReadINIStr $StatusState "$StatusPath" "status" "state"
  ReadINIStr $Progress "$StatusPath" "status" "progress"
  ReadINIStr $StatusPhase "$StatusPath" "status" "phase"
  ReadINIStr $StatusAction "$StatusPath" "status" "action"
  ReadINIStr $StatusResultKind "$StatusPath" "status" "result_kind"
  ReadINIStr $StatusWin32Code "$StatusPath" "status" "win32_code"
  ReadINIStr $StatusMsiCode "$StatusPath" "status" "msi_code"
  ReadINIStr $StatusLogPath "$StatusPath" "status" "log_path"
  ${If} ${Errors}
    Return
  ${EndIf}
  ${If} $StatusLogPath != ""
    StrCpy $InstallLog $StatusLogPath
  ${EndIf}
  !insertmacro RecordPreviewAction "status:$StatusState:$Progress"
  ClearErrors
  Return
ReadInstallerStatusMissing:
  ClearErrors
FunctionEnd

!ifndef PREVIEW_MODE
Function ShowFilesInUseChoices
  ShowWindow $CancelHit ${SW_HIDE}
  !insertmacro ShowControl $FilesRetryHit
  !insertmacro ShowControl $FilesContinueHit
  !insertmacro ShowControl $FilesCancelHit
  !insertmacro PrepareOverlay $FilesRetryHit
  !insertmacro PrepareOverlay $FilesContinueHit
  !insertmacro PrepareOverlay $FilesCancelHit
FunctionEnd
!endif

Function HideFilesInUseChoices
  !insertmacro HideControl $FilesRetryHit
  !insertmacro HideControl $FilesContinueHit
  !insertmacro HideControl $FilesCancelHit
  ${If} $InstallActive == 1
  ${AndIf} $CancelRequested == 0
    ShowWindow $CancelHit ${SW_SHOW}
  ${EndIf}
FunctionEnd

Function StartInstall
  StrCpy $Progress 0
  StrCpy $LastStage -1
  StrCpy $InstallTick 0
  StrCpy $InstallExitCode 0
  StrCpy $InstallFailureKind ""
  StrCpy $PreviewAnimating 0
  StrCpy $InstallActive 0
  StrCpy $BrokerStarted 0
  StrCpy $CancelRequested 0
  StrCpy $CommandSequence 0
  StrCpy $StatusSequence 0
  StrCpy $StatusState ""
  Call CleanupInstallerSession
  Call ShowInstalling
  Call UpdateProgress
  System::Call 'user32::UpdateWindow(p $HWNDPARENT) i .r0'

!ifdef PREVIEW_MODE
  StrCpy $PreviewAnimating 1
  ${NSD_CreateTimer} PollInstallation 85
  Return
!else
  Call CreateInstallerSession
  ${If} ${Errors}
    StrCpy $InstallExitCode $SessionErrorCode
    ${If} $InstallExitCode == 0
      StrCpy $InstallExitCode 5
    ${EndIf}
    StrCpy $InstallFailureKind "session"
    Call ShowError
    Return
  ${EndIf}
  Call ExtractMsi
  ${If} ${Errors}
    StrCpy $InstallExitCode $SessionErrorCode
    StrCpy $InstallFailureKind "payload_extract"
    Call CleanupInstallerSession
    Call ShowError
    Return
  ${EndIf}
  ClearErrors
  Exec '"$SessionDir\mediadrop-installer-worker.exe" --broker --session-dir "$SessionDir"'
  ${If} ${Errors}
    System::Call 'kernel32::GetLastError() i .r0'
    StrCpy $InstallExitCode $0
    ${If} $InstallExitCode == 0
      StrCpy $InstallExitCode 2
    ${EndIf}
    StrCpy $InstallFailureKind "broker_launch"
    Call CleanupInstallerSession
    Call ShowError
    Return
  ${EndIf}
  StrCpy $BrokerStarted 1
  StrCpy $InstallActive 1
  ${NSD_CreateTimer} PollInstallation 100
!endif
FunctionEnd

Function PollInstallation
!ifdef PREVIEW_MODE
  ${If} $PreviewAnimating == 1
    IntOp $Progress $Progress + 1
    Call UpdateProgress
    ${If} $Progress >= 100
      StrCpy $PreviewAnimating 0
      Sleep 220
      Call ShowDone
    ${EndIf}
  ${EndIf}
  Return
!else
  ${If} $InstallActive != 1
    Return
  ${EndIf}
  IntOp $InstallTick $InstallTick + 1
  Call ReadInstallerStatus
  ${If} ${Errors}
    StrCpy $InstallActive 0
    StrCpy $InstallExitCode 13
    StrCpy $InstallFailureKind "status_invalid"
    Call ShowError
    Return
  ${EndIf}
  ${If} $InstallTick > 1500
    StrCpy $InstallActive 0
    StrCpy $InstallExitCode 1460
    StrCpy $InstallFailureKind "status_timeout"
    Call ShowError
    Return
  ${EndIf}
  ${If} $StatusChanged == 0
    Return
  ${EndIf}

  ${If} $StatusAction != ""
    ${NSD_SetText} $ProgressCurrent $StatusAction
  ${ElseIf} $StatusPhase != ""
    ${NSD_SetText} $ProgressCurrent $StatusPhase
  ${EndIf}
  Call UpdateProgress

  ${If} $StatusState == "files_in_use"
    Call ShowFilesInUseChoices
    Return
  ${EndIf}
  Call HideFilesInUseChoices
  ${If} $StatusState == "cancel_pending"
  ${OrIf} $StatusState == "rolling_back"
    StrCpy $CancelRequested 1
    ShowWindow $CancelHit ${SW_HIDE}
    Return
  ${EndIf}
  ${If} $StatusState == "succeeded"
    StrCpy $InstallActive 0
    StrCpy $BrokerStarted 0
    ${NSD_KillTimer} PollInstallation
    StrCpy $Progress 100
    Call UpdateProgress
    ReadINIStr $0 "$StatusPath" "status" "reboot_required"
    ${If} $0 == 1
      SetRebootFlag true
    ${EndIf}
    Call CleanupInstallerSession
    ${If} $ExtensionSetup == 1
      Call ShowExtensionSetup
    ${Else}
      Call ShowDone
    ${EndIf}
    Return
  ${EndIf}
  ${If} $StatusState == "failed"
  ${OrIf} $StatusState == "elevation_cancelled"
    StrCpy $InstallActive 0
    StrCpy $BrokerStarted 0
    ${NSD_KillTimer} PollInstallation
    StrCpy $InstallFailureKind $StatusResultKind
    StrCpy $InstallExitCode $StatusMsiCode
    ${If} $InstallExitCode == 0
      StrCpy $InstallExitCode $StatusWin32Code
    ${EndIf}
    ${If} $InstallExitCode == 0
      StrCpy $InstallExitCode 1
    ${EndIf}
    Call CleanupInstallerSession
    Call ShowError
  ${EndIf}
!endif
FunctionEnd

Function StopInstallation
!ifdef PREVIEW_MODE
  ${NSD_KillTimer} PollInstallation
  StrCpy $PreviewAnimating 0
!else
  ${If} $InstallActive == 1
  ${AndIf} $CancelRequested == 0
    Push "cancel"
    Call SendBrokerCommand
    ${IfNot} ${Errors}
      StrCpy $CancelRequested 1
      ShowWindow $CancelHit ${SW_HIDE}
      ${NSD_SetText} $ProgressCurrent "Windows Installer değişiklikleri güvenle geri alıyor"
    ${EndIf}
  ${EndIf}
!endif
FunctionEnd

Function RequestExit
  StrCpy $ExitRequested 1
  GetDlgItem $0 $HWNDPARENT 1
  EnableWindow $0 1
  SendMessage $0 ${BM_CLICK} 0 0
FunctionEnd

Function ApplyToggleFrame
  Exch $0
  Push $1
  StrCpy $1 "$PLUGINSDIR\ui\toggle-$0.bmp"
  ${If} $ToggleTarget == "welcome"
    ${NSD_FreeBitmap} $WelcomeToggleImage
    ${NSD_SetBitmap} $WelcomeToggle "$1" $WelcomeToggleImage
    System::Call 'user32::UpdateWindow(p $WelcomeToggle) i .r2'
  ${ElseIf} $ToggleTarget == "launch"
    ${NSD_FreeBitmap} $LaunchToggleImage
    ${NSD_SetBitmap} $LaunchToggle "$1" $LaunchToggleImage
    System::Call 'user32::UpdateWindow(p $LaunchToggle) i .r2'
  ${ElseIf} $ToggleTarget == "done_extension"
    ${NSD_FreeBitmap} $ExtensionToggleImage
    ${NSD_SetBitmap} $ExtensionToggle "$1" $ExtensionToggleImage
    System::Call 'user32::UpdateWindow(p $ExtensionToggle) i .r2'
  ${EndIf}
  Pop $1
  Pop $0
FunctionEnd

Function AnimateToggle
  Exch $0
  Push $1
  ${If} $MotionEnabled == 0
    ${If} $0 == 1
      StrCpy $1 6
    ${Else}
      StrCpy $1 0
    ${EndIf}
    Push $1
    Call ApplyToggleFrame
    Goto AnimateToggleDone
  ${EndIf}

  ${If} $0 == 1
    StrCpy $1 0
    AnimateToggleOn:
      IntOp $1 $1 + 1
      Push $1
      Call ApplyToggleFrame
      ${If} $1 < 6
        Sleep 16
        Goto AnimateToggleOn
      ${EndIf}
  ${Else}
    StrCpy $1 6
    AnimateToggleOff:
      IntOp $1 $1 - 1
      Push $1
      Call ApplyToggleFrame
      ${If} $1 > 0
        Sleep 16
        Goto AnimateToggleOff
      ${EndIf}
  ${EndIf}

  AnimateToggleDone:
  !insertmacro RecordPreviewAction "toggle_motion_complete"
  Pop $1
  Pop $0
FunctionEnd

Function ToggleWelcomeExtension
  Pop $0
  !insertmacro RecordPreviewAction "toggle_welcome_extension"
  IntOp $ExtensionSetup $ExtensionSetup ^ 1
  StrCpy $ExtensionHandled 0
  StrCpy $ToggleTarget "welcome"
  Push $ExtensionSetup
  Call AnimateToggle
FunctionEnd

Function ToggleLaunchApp
  Pop $0
  !insertmacro RecordPreviewAction "toggle_launch_app"
  IntOp $LaunchApp $LaunchApp ^ 1
  StrCpy $ToggleTarget "launch"
  Push $LaunchApp
  Call AnimateToggle
FunctionEnd

Function ToggleDoneExtension
  Pop $0
  !insertmacro RecordPreviewAction "toggle_done_extension"
  IntOp $ExtensionSetup $ExtensionSetup ^ 1
  StrCpy $ExtensionHandled 0
  StrCpy $ToggleTarget "done_extension"
  Push $ExtensionSetup
  Call AnimateToggle
FunctionEnd

Function OnStart
  Pop $0
  !insertmacro RecordPreviewAction "start"
  Call StartInstall
FunctionEnd

Function OnCancel
  Pop $0
  !insertmacro RecordPreviewAction "cancel"
  Call StopInstallation
!ifdef PREVIEW_MODE
  StrCpy $InstallExitCode 1602
  StrCpy $InstallFailureKind "msi"
  Call ShowError
!endif
FunctionEnd

Function OnRetry
  Pop $0
  !insertmacro RecordPreviewAction "retry"
  ${If} $BrokerStarted != 0
    Return
  ${EndIf}
  Call CleanupInstallerSession
!ifdef LIFECYCLE_TEST_MODE
  StrCpy $TestWorkerScenario "success"
!endif
  Call StartInstall
FunctionEnd

Function OnGiveUp
  Pop $0
  !insertmacro RecordPreviewAction "give_up"
  ${If} $BrokerStarted != 0
    Call StopInstallation
  ${Else}
    Call CleanupInstallerSession
  ${EndIf}
  Call RequestExit
FunctionEnd

Function OnFilesRetry
  Pop $0
  Push "retry_files"
  Call SendBrokerCommand
  Call HideFilesInUseChoices
FunctionEnd

Function OnFilesContinue
  Pop $0
  Push "continue_files"
  Call SendBrokerCommand
  Call HideFilesInUseChoices
FunctionEnd

Function OnFilesCancel
  Pop $0
  Push "cancel_files"
  Call SendBrokerCommand
  StrCpy $CancelRequested 1
  Call HideFilesInUseChoices
  ShowWindow $CancelHit ${SW_HIDE}
  ${NSD_SetText} $ProgressCurrent "Windows Installer değişiklikleri güvenle geri alıyor"
FunctionEnd

Function OnMinimize
  Pop $0
  !insertmacro RecordPreviewAction "minimize"
  SendMessage $HWNDPARENT ${WM_SYSCOMMAND} 0xF020 0
FunctionEnd

Function OnClose
  Pop $0
  !insertmacro RecordPreviewAction "close"
  ${If} $CurrentScreen == "installing"
    Call StopInstallation
  ${ElseIf} $CurrentScreen == "done"
    Call FinishSetup
  ${Else}
    ${If} $BrokerStarted != 0
      Call StopInstallation
    ${Else}
      Call CleanupInstallerSession
    ${EndIf}
    Call RequestExit
  ${EndIf}
FunctionEnd

Function OpenInstallLog
  Pop $0
  !insertmacro RecordPreviewAction "open_log"
!ifdef UI_TEST_MODE
  Return
!else
  IfFileExists "$InstallLog" 0 +2
    ExecShell "open" "$InstallLog"
!endif
FunctionEnd

!ifndef UI_TEST_MODE
Function ResolveMediaDropExecutable
  SetRegView 64
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\mediadrop.exe" ""
  ${If} $0 == ""
    ReadRegStr $0 HKLM "Software\Microsoft\Windows\CurrentVersion\App Paths\mediadrop.exe" ""
  ${EndIf}
  ${If} $0 == ""
    StrCpy $0 "$PROGRAMFILES64\MediaDrop\mediadrop.exe"
  ${EndIf}
FunctionEnd
!endif

Function OnFinish
  Pop $0
  !insertmacro RecordPreviewAction "finish"
  Call FinishSetup
FunctionEnd

Function FinishSetup
!ifdef UI_TEST_MODE
  ${If} $ExtensionSetup == 1
  ${AndIf} $ExtensionHandled == 0
    Call ShowExtensionSetup
    Return
  ${EndIf}
  Call RequestExit
!else
  Call CleanupInstallerSession
  ${If} $ExtensionSetup == 1
  ${AndIf} $ExtensionHandled == 0
    Call ShowExtensionSetup
    Return
  ${EndIf}
  Call ResolveMediaDropExecutable
  ${If} $LaunchApp == 1
    IfFileExists "$0" 0 +2
      Exec '"$0"'
  ${EndIf}
  Call RequestExit
!endif
FunctionEnd

Function ApplyHoverFrame
  ${NSD_FreeBitmap} $HoverBitmap
  ${NSD_SetBitmap} $HoverVisual "$PLUGINSDIR\ui\hover-$HoverTarget-$HoverProgress.bmp" $HoverBitmap
  System::Call 'user32::SetWindowPos(p $HoverVisual, p 0, i $HoverX, i $HoverY, i $HoverWidth, i $HoverHeight, i 0x14) i .r0'
  ShowWindow $HoverVisual ${SW_SHOW}
FunctionEnd

Function ApplyDynamicHover
  Call ApplyHoverFrame
  ${If} $HoverTarget == "browser_0"
    System::Call 'user32::SetWindowPos(p $ExtensionBrowserHit0, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "browser_1"
    System::Call 'user32::SetWindowPos(p $ExtensionBrowserHit1, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "browser_2"
    System::Call 'user32::SetWindowPos(p $ExtensionBrowserHit2, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "browser_3"
    System::Call 'user32::SetWindowPos(p $ExtensionBrowserHit3, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "extension_primary"
    System::Call 'user32::SetWindowPos(p $ExtensionPrimaryHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "extension_copy"
    System::Call 'user32::SetWindowPos(p $ExtensionCopyHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "extension_reveal"
    System::Call 'user32::SetWindowPos(p $ExtensionRevealHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "minimize"
    System::Call 'user32::SetWindowPos(p $MinimizeHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "close"
    System::Call 'user32::SetWindowPos(p $CloseHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "start"
    System::Call 'user32::SetWindowPos(p $StartHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "cancel"
    System::Call 'user32::SetWindowPos(p $CancelHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "extension_later"
    System::Call 'user32::SetWindowPos(p $ExtensionLaterHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "summary"
    System::Call 'user32::SetWindowPos(p $SummaryHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "finish"
    System::Call 'user32::SetWindowPos(p $FinishHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "log"
    System::Call 'user32::SetWindowPos(p $LogHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "give_up"
    System::Call 'user32::SetWindowPos(p $GiveUpHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${ElseIf} $HoverTarget == "retry"
    System::Call 'user32::SetWindowPos(p $RetryHit, p 0, i 0, i 0, i 0, i 0, i 0x13) i .r0'
  ${EndIf}
FunctionEnd

Function UpdateHoverFeedback
  StrCpy $3 ""
  StrCpy $4 0
  StrCpy $5 0
  StrCpy $6 0
  StrCpy $7 0

  !insertmacro MatchHoverRect "minimize" 1022 13 1060 51
  !insertmacro MatchHoverRect "close" 1064 13 1102 51

  ${If} $CurrentScreen == "welcome"
    !insertmacro MatchHoverRect "start" 884 566 1064 612
  ${ElseIf} $CurrentScreen == "installing"
    !insertmacro MatchHoverRect "cancel" 981 566 1064 612
  ${ElseIf} $CurrentScreen == "extension"
    ${If} $BrowserCount > 0
      !insertmacro MatchHoverRect "browser_0" 482 228 618 273
    ${EndIf}
    ${If} $BrowserCount > 1
      !insertmacro MatchHoverRect "browser_1" 630 228 766 273
    ${EndIf}
    ${If} $BrowserCount > 2
      !insertmacro MatchHoverRect "browser_2" 778 228 914 273
    ${EndIf}
    ${If} $BrowserCount > 3
      !insertmacro MatchHoverRect "browser_3" 926 228 1062 273
    ${EndIf}
    !insertmacro MatchHoverRect "extension_later" 482 566 598 612
    !insertmacro MatchHoverRect "extension_copy" 610 566 742 612
    !insertmacro MatchHoverRect "extension_reveal" 752 566 884 612
    !insertmacro MatchHoverRect "extension_primary" 894 566 1064 612
  ${ElseIf} $CurrentScreen == "done"
    !insertmacro MatchHoverRect "summary" 482 566 608 612
    !insertmacro MatchHoverRect "finish" 884 566 1064 612
  ${ElseIf} $CurrentScreen == "error"
    !insertmacro MatchHoverRect "log" 482 566 628 612
    !insertmacro MatchHoverRect "give_up" 789 566 874 612
    !insertmacro MatchHoverRect "retry" 884 566 1064 612
  ${EndIf}

  ${If} $3 == ""
    ${If} $HoverTarget != ""
      ShowWindow $HoverVisual ${SW_HIDE}
      StrCpy $HoverTarget ""
      StrCpy $HoverProgress 0
      Call RefreshDynamicUi
    ${EndIf}
    Return
  ${EndIf}

  ${If} $3 != $HoverTarget
    ${If} $HoverTarget != ""
      ShowWindow $HoverVisual ${SW_HIDE}
      Call RefreshDynamicUi
    ${EndIf}
    StrCpy $HoverTarget $3
    StrCpy $HoverX $4
    StrCpy $HoverY $5
    StrCpy $HoverWidth $6
    StrCpy $HoverHeight $7
    ${If} $MotionEnabled == 0
      StrCpy $HoverProgress 4
    ${Else}
      StrCpy $HoverProgress 1
    ${EndIf}
    Call ApplyDynamicHover
  ${ElseIf} $HoverProgress < 4
    IntOp $HoverProgress $HoverProgress + 1
    Call ApplyDynamicHover
  ${EndIf}
FunctionEnd

Function DragTimer
  System::Alloc 8
  Pop $0
  System::Call 'user32::GetCursorPos(p $0) i .r1'
  System::Call 'user32::ScreenToClient(p $HWNDPARENT, p $0) i .r1'
  System::Call '*$0(i .r1, i .r2)'
  System::Free $0
  StrCpy $HoverMouseX $1
  StrCpy $HoverMouseY $2
!ifdef UI_TEST_MODE
  ${If} $PreviewHoverTest == "install"
    StrCpy $HoverMouseX 974
    StrCpy $HoverMouseY 589
  ${ElseIf} $PreviewHoverTest == "browser_1"
    StrCpy $HoverMouseX 698
    StrCpy $HoverMouseY 250
  ${ElseIf} $PreviewHoverTest == "minimize"
    StrCpy $HoverMouseX 1041
    StrCpy $HoverMouseY 32
  ${ElseIf} $PreviewHoverTest == "extension_primary"
    StrCpy $HoverMouseX 979
    StrCpy $HoverMouseY 589
  ${EndIf}
!endif
  Call UpdateHoverFeedback

  ${If} $Dragging == 1
    Return
  ${EndIf}
  System::Call 'user32::GetAsyncKeyState(i 1) i .r0'
  IntOp $0 $0 & 0x8000
  ${If} $0 == 0
    Return
  ${EndIf}
  ${If} $HoverMouseX >= 0
  ${AndIf} $HoverMouseX < 1010
  ${AndIf} $HoverMouseY >= 0
  ${AndIf} $HoverMouseY < 64
    StrCpy $Dragging 1
    ${NSD_KillTimer} DragTimer
    System::Call 'user32::ReleaseCapture() i .r0'
    SendMessage $HWNDPARENT ${WM_NCLBUTTONDOWN} 2 0
    ${NSD_CreateTimer} DragTimer 25
    StrCpy $Dragging 0
  ${EndIf}
FunctionEnd

!ifdef UI_TEST_MODE
Function KeepPreviewHidden
  ${If} $PreviewHidden == 1
    ShowWindow $HWNDPARENT ${SW_HIDE}
  ${EndIf}
FunctionEnd
!endif

Function SetupPage
  Call ExtractUiAssets
  Call RegisterFonts
  Call ConfigureWindow

  nsDialogs::Create 1018
  Pop $Dialog
  ${If} $Dialog == error
    Abort
  ${EndIf}
  Call HideNativeControls
  System::Call 'user32::SetWindowPos(p $Dialog, p 0, i 0, i 0, i 1120, i 650, i 0x10) i .r0'

  ${NSD_CreateBitmap} 0 0 1120 650 ""
  Pop $Background

  ${NSD_CreateBitmap} 1018 502 46 26 ""
  Pop $WelcomeToggle
  !insertmacro PrepareOverlay $WelcomeToggle
  ${NSD_SetBitmap} $WelcomeToggle "$PLUGINSDIR\ui\toggle-0.bmp" $WelcomeToggleImage
  ${NSD_CreateBitmap} 1018 377 46 26 ""
  Pop $LaunchToggle
  !insertmacro PrepareOverlay $LaunchToggle
  ${NSD_SetBitmap} $LaunchToggle "$PLUGINSDIR\ui\toggle-6.bmp" $LaunchToggleImage
  ${NSD_CreateBitmap} 1018 436 46 26 ""
  Pop $ExtensionToggle
  !insertmacro PrepareOverlay $ExtensionToggle
  ${NSD_SetBitmap} $ExtensionToggle "$PLUGINSDIR\ui\toggle-0.bmp" $ExtensionToggleImage

  ${NSD_CreateBitmap} 128 169 170 170 ""
  Pop $Logo
  !insertmacro PrepareOverlay $Logo
  ${NSD_CreateLabel} 482 300 12 10 ""
  Pop $ProgressFill
  !insertmacro PrepareOverlay $ProgressFill
  SetCtlColors $ProgressFill 0xDFA326 0xDFA326
  ${NSD_CreateLabel} 482 238 220 62 "0%"
  Pop $ProgressNumber
  !insertmacro PrepareOverlay $ProgressNumber
  SetCtlColors $ProgressNumber 0xF6F5F1 0x17181C
  SendMessage $ProgressNumber ${WM_SETFONT} $FontPercent 1
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${SS_RIGHT} 0 804 247 260 42 "Kurulum dosyaları hazırlanıyor"
  Pop $ProgressCurrent
  SetCtlColors $ProgressCurrent 0xCCCBC5 0x17181C
  SendMessage $ProgressCurrent ${WM_SETFONT} $FontSemibold 1

  ${NSD_CreateLabel} 482 340 22 28 "●"
  Pop $StageDot0
  !insertmacro PrepareOverlay $StageDot0
  ${NSD_CreateLabel} 482 389 22 28 "●"
  Pop $StageDot1
  !insertmacro PrepareOverlay $StageDot1
  ${NSD_CreateLabel} 482 438 22 28 "●"
  Pop $StageDot2
  !insertmacro PrepareOverlay $StageDot2
  ${NSD_CreateLabel} 482 487 22 28 "●"
  Pop $StageDot3
  !insertmacro PrepareOverlay $StageDot3
  ${NSD_CreateLabel} 514 340 410 28 "Kurulum paketi doğrulanıyor"
  Pop $StageName0
  !insertmacro PrepareOverlay $StageName0
  ${NSD_CreateLabel} 514 389 410 28 "Windows Installer hazırlanıyor"
  Pop $StageName1
  !insertmacro PrepareOverlay $StageName1
  ${NSD_CreateLabel} 514 438 410 28 "MediaDrop bileşenleri kuruluyor"
  Pop $StageName2
  !insertmacro PrepareOverlay $StageName2
  ${NSD_CreateLabel} 514 487 410 28 "Kurulum sonucu doğrulanıyor"
  Pop $StageName3
  !insertmacro PrepareOverlay $StageName3
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${SS_RIGHT} 0 970 340 94 28 "BEKLİYOR"
  Pop $StageState0
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${SS_RIGHT} 0 970 389 94 28 "BEKLİYOR"
  Pop $StageState1
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${SS_RIGHT} 0 970 438 94 28 "BEKLİYOR"
  Pop $StageState2
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${SS_RIGHT} 0 970 487 94 28 "BEKLİYOR"
  Pop $StageState3

  SendMessage $StageDot0 ${WM_SETFONT} $FontSmall 1
  SendMessage $StageDot1 ${WM_SETFONT} $FontSmall 1
  SendMessage $StageDot2 ${WM_SETFONT} $FontSmall 1
  SendMessage $StageDot3 ${WM_SETFONT} $FontSmall 1
  SendMessage $StageName0 ${WM_SETFONT} $FontNormal 1
  SendMessage $StageName1 ${WM_SETFONT} $FontNormal 1
  SendMessage $StageName2 ${WM_SETFONT} $FontNormal 1
  SendMessage $StageName3 ${WM_SETFONT} $FontNormal 1
  SendMessage $StageState0 ${WM_SETFONT} $FontSmall 1
  SendMessage $StageState1 ${WM_SETFONT} $FontSmall 1
  SendMessage $StageState2 ${WM_SETFONT} $FontSmall 1
  SendMessage $StageState3 ${WM_SETFONT} $FontSmall 1

  ${NSD_CreateLabel} 482 279 582 60 ""
  Pop $ErrorLead
  !insertmacro PrepareOverlay $ErrorLead
  SetCtlColors $ErrorLead 0xAAA9A4 transparent
  SendMessage $ErrorLead ${WM_SETFONT} $FontNormal 1
  ${NSD_CreateLabel} 500 368 540 24 ""
  Pop $ErrorTitle
  !insertmacro PrepareOverlay $ErrorTitle
  SetCtlColors $ErrorTitle 0xFFB7C2 transparent
  SendMessage $ErrorTitle ${WM_SETFONT} $FontSemibold 1
  ${NSD_CreateLabel} 500 398 540 45 ""
  Pop $ErrorDetail
  !insertmacro PrepareOverlay $ErrorDetail
  SetCtlColors $ErrorDetail 0xD5B3B9 transparent
  SendMessage $ErrorDetail ${WM_SETFONT} $FontNormal 1

  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${WS_TABSTOP}|${SS_NOTIFY}|${SS_CENTER}|${SS_CENTERIMAGE} ${WS_EX_TRANSPARENT} 482 228 136 45 ""
  Pop $ExtensionBrowserHit0
  SetCtlColors $ExtensionBrowserHit0 0xCCCBC5 transparent
  SendMessage $ExtensionBrowserHit0 ${WM_SETFONT} $FontSemibold 1
  ${NSD_OnClick} $ExtensionBrowserHit0 SelectExtensionBrowser0
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${WS_TABSTOP}|${SS_NOTIFY}|${SS_CENTER}|${SS_CENTERIMAGE} ${WS_EX_TRANSPARENT} 630 228 136 45 ""
  Pop $ExtensionBrowserHit1
  SetCtlColors $ExtensionBrowserHit1 0xCCCBC5 transparent
  SendMessage $ExtensionBrowserHit1 ${WM_SETFONT} $FontSemibold 1
  ${NSD_OnClick} $ExtensionBrowserHit1 SelectExtensionBrowser1
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${WS_TABSTOP}|${SS_NOTIFY}|${SS_CENTER}|${SS_CENTERIMAGE} ${WS_EX_TRANSPARENT} 778 228 136 45 ""
  Pop $ExtensionBrowserHit2
  SetCtlColors $ExtensionBrowserHit2 0xCCCBC5 transparent
  SendMessage $ExtensionBrowserHit2 ${WM_SETFONT} $FontSemibold 1
  ${NSD_OnClick} $ExtensionBrowserHit2 SelectExtensionBrowser2
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${WS_TABSTOP}|${SS_NOTIFY}|${SS_CENTER}|${SS_CENTERIMAGE} ${WS_EX_TRANSPARENT} 926 228 136 45 ""
  Pop $ExtensionBrowserHit3
  SetCtlColors $ExtensionBrowserHit3 0xCCCBC5 transparent
  SendMessage $ExtensionBrowserHit3 ${WM_SETFONT} $FontSemibold 1
  ${NSD_OnClick} $ExtensionBrowserHit3 SelectExtensionBrowser3

  ${NSD_CreateLabel} 541 314 485 24 ""
  Pop $ExtensionStep0
  SetCtlColors $ExtensionStep0 0xCCCBC5 0x222328
  SendMessage $ExtensionStep0 ${WM_SETFONT} $FontNormal 1
  ${NSD_CreateLabel} 541 364 485 24 ""
  Pop $ExtensionStep1
  SetCtlColors $ExtensionStep1 0xCCCBC5 0x222328
  SendMessage $ExtensionStep1 ${WM_SETFONT} $FontNormal 1
  ${NSD_CreateLabel} 541 414 485 24 ""
  Pop $ExtensionStep2
  SetCtlColors $ExtensionStep2 0xCCCBC5 0x222328
  SendMessage $ExtensionStep2 ${WM_SETFONT} $FontNormal 1
  ${NSD_CreateLabel} 526 488 510 22 ""
  Pop $ExtensionStatusTitle
  SetCtlColors $ExtensionStatusTitle 0xF6F5F1 0x202126
  SendMessage $ExtensionStatusTitle ${WM_SETFONT} $FontSemibold 1
  ${NSD_CreateLabel} 498 489 20 20 "●"
  Pop $ExtensionStatusDot
  SetCtlColors $ExtensionStatusDot 0xF5C75D 0x202126
  SendMessage $ExtensionStatusDot ${WM_SETFONT} $FontSmall 1
  ${NSD_CreateLabel} 526 515 510 18 ""
  Pop $ExtensionStatusDetail
  SetCtlColors $ExtensionStatusDetail 0x8F8F8B 0x202126
  SendMessage $ExtensionStatusDetail ${WM_SETFONT} $FontSmall 1

  ${NSD_CreateLabel} 480 226 140 49 ""
  Pop $ExtensionBrowserMask0
  SetCtlColors $ExtensionBrowserMask0 0x17181C 0x17181C
  ${NSD_CreateLabel} 628 226 140 49 ""
  Pop $ExtensionBrowserMask1
  SetCtlColors $ExtensionBrowserMask1 0x17181C 0x17181C
  ${NSD_CreateLabel} 776 226 140 49 ""
  Pop $ExtensionBrowserMask2
  SetCtlColors $ExtensionBrowserMask2 0x17181C 0x17181C
  ${NSD_CreateLabel} 924 226 140 49 ""
  Pop $ExtensionBrowserMask3
  SetCtlColors $ExtensionBrowserMask3 0x17181C 0x17181C

  ${NSD_CreateBitmap} 0 0 1 1 ""
  Pop $HoverVisual
  ShowWindow $HoverVisual ${SW_HIDE}
  !insertmacro PrepareOverlay $HoverVisual

  !insertmacro CreateHit $MinimizeHit 1022 13 38 38 "Küçült" 0x101114 OnMinimize
  !insertmacro CreateHit $CloseHit 1064 13 38 38 "Kapat" 0x101114 OnClose
  !insertmacro CreateHit $StartHit 884 566 180 46 "Kuruluma başla" 0xF5C75D OnStart
  !insertmacro CreateHit $CancelHit 981 566 83 46 "İptal et" 0x25262B OnCancel
  !insertmacro CreateTextButton $FilesRetryHit 662 566 124 46 "Yeniden dene" 0xF6F5F1 0x25262B OnFilesRetry
  !insertmacro CreateTextButton $FilesContinueHit 796 566 154 46 "Yine de sürdür" 0x171512 0xF5C75D OnFilesContinue
  !insertmacro CreateTextButton $FilesCancelHit 960 566 104 46 "İptal et" 0xFFB7C2 0x3B2229 OnFilesCancel
  !insertmacro CreateHit $SummaryHit 482 566 126 46 "Kurulum özeti" 0x25262B OpenInstallLog
  !insertmacro CreateHit $FinishHit 884 566 180 46 "Bitir" 0xF5C75D OnFinish
  !insertmacro CreateHit $LogHit 482 566 146 46 "Kurulum günlüğü" 0x25262B OpenInstallLog
  !insertmacro CreateHit $GiveUpHit 789 566 85 46 "Vazgeç" 0x3B2229 OnGiveUp
  !insertmacro CreateHit $RetryHit 884 566 180 46 "Tekrar dene" 0xF5C75D OnRetry
  !insertmacro CreateHit $WelcomeToggleHit 482 487 582 54 "Tarayıcı eklentisini bağla" 0x17181C ToggleWelcomeExtension
  !insertmacro CreateHit $LaunchToggleHit 482 355 582 59 "MediaDrop'u şimdi aç" 0x17181C ToggleLaunchApp
  !insertmacro CreateHit $ExtensionToggleHit 482 414 582 59 "Tarayıcı eklentisini bağla" 0x17181C ToggleDoneExtension
  !insertmacro CreateHit $ExtensionLaterHit 482 566 116 46 "Daha sonra" 0x25262B OnExtensionLater
  !insertmacro CreateOverlayButton $ExtensionCopyHit 610 566 132 46 "Yolu kopyala" 0xCCCBC5 OnExtensionCopyPath
  !insertmacro CreateOverlayButton $ExtensionRevealHit 752 566 132 46 "Klasörü göster" 0xCCCBC5 OnExtensionRevealPath
  !insertmacro CreateOverlayButton $ExtensionPrimaryHit 894 566 170 46 "Tarayıcıyı aç" 0x171512 OnExtensionPrimary

  ${NSD_CreateTimer} DragTimer 25

!ifdef PREVIEW_MODE
  ${If} $PreviewScreen == "installing"
    StrCpy $Progress 58
    StrCpy $LastStage -1
    Call ShowInstalling
    Call UpdateProgress
  ${ElseIf} $PreviewScreen == "extension"
    Call ShowExtensionSetup
  ${ElseIf} $PreviewScreen == "done"
    Call ShowDone
  ${ElseIf} $PreviewScreen == "error"
    StrCpy $InstallExitCode 1603
    Call ShowError
  ${Else}
    Call ShowWelcome
  ${EndIf}
!else
  ${If} $StartedElevated == 1
    StrCpy $InstallFailureKind "outer_elevated"
    StrCpy $InstallExitCode 740
    Call ShowError
  ${Else}
    Call ShowWelcome
  ${EndIf}
!endif

  !insertmacro RecordPreviewAction "startup_ready"
  ShowWindow $HWNDPARENT ${SW_SHOW}
  System::Call 'user32::UpdateWindow(p $HWNDPARENT) i .r0'

!ifdef UI_TEST_MODE
  ${If} $PreviewHidden == 1
    ShowWindow $HWNDPARENT ${SW_HIDE}
    ${NSD_CreateTimer} KeepPreviewHidden 25
  ${EndIf}
!endif
  nsDialogs::Show
  ${If} $ExitRequested == 1
    Quit
  ${EndIf}
FunctionEnd

Function .onGUIEnd
  ${NSD_KillTimer} DragTimer
  ${NSD_KillTimer} PollInstallation
  ${NSD_KillTimer} PollExtensionConnection
!ifdef UI_TEST_MODE
  ${NSD_KillTimer} KeepPreviewHidden
!endif
  ${NSD_FreeBitmap} $BackgroundImage
  ${NSD_FreeBitmap} $WelcomeToggleImage
  ${NSD_FreeBitmap} $LaunchToggleImage
  ${NSD_FreeBitmap} $ExtensionToggleImage
  ${NSD_FreeBitmap} $HoverBitmap
  ${NSD_FreeBitmap} $LogoImage
  ${If} $ExtensionEvent != 0
    System::Call 'kernel32::CloseHandle(p $ExtensionEvent) i .r0'
    StrCpy $ExtensionEvent 0
  ${EndIf}
  ${If} $SetupMutex != 0
    System::Call 'kernel32::CloseHandle(p $SetupMutex) i .r0'
    StrCpy $SetupMutex 0
  ${EndIf}
  System::Call 'gdi32::DeleteObject(p $FontNormal) i .r0'
  System::Call 'gdi32::DeleteObject(p $FontSemibold) i .r0'
  System::Call 'gdi32::DeleteObject(p $FontSmall) i .r0'
  System::Call 'gdi32::DeleteObject(p $FontPercent) i .r0'
  System::Call 'gdi32::RemoveFontResourceExW(w "$PLUGINSDIR\ui\InstrumentSans-Regular.ttf", i 0x10, p 0) i .r0'
  System::Call 'gdi32::RemoveFontResourceExW(w "$PLUGINSDIR\ui\InstrumentSans-Medium.ttf", i 0x10, p 0) i .r0'
  System::Call 'gdi32::RemoveFontResourceExW(w "$PLUGINSDIR\ui\InstrumentSans-SemiBold.ttf", i 0x10, p 0) i .r0'
  System::Call 'gdi32::RemoveFontResourceExW(w "$PLUGINSDIR\ui\InstrumentSans-Bold.ttf", i 0x10, p 0) i .r0'
FunctionEnd

Function RunSilentInstall
  StrCpy $SilentMode 1
  ${If} $StartedElevated == 1
    SetErrorLevel 740
    Return
  ${EndIf}
  StrCpy $InstallTick 0
  StrCpy $InstallExitCode 0
  StrCpy $CommandSequence 0
  StrCpy $StatusSequence 0
  StrCpy $BrokerStarted 0
  Call CreateInstallerSession
  ${If} ${Errors}
    StrCpy $InstallExitCode $SessionErrorCode
    ${If} $InstallExitCode == 0
      StrCpy $InstallExitCode 5
    ${EndIf}
    Goto SilentInstallDone
  ${EndIf}
  Call ExtractMsi
  ${If} ${Errors}
    StrCpy $InstallExitCode $SessionErrorCode
    Goto SilentInstallDone
  ${EndIf}
  ClearErrors
  Exec '"$SessionDir\mediadrop-installer-worker.exe" --broker --session-dir "$SessionDir"'
  ${If} ${Errors}
    System::Call 'kernel32::GetLastError() i .r0'
    StrCpy $InstallExitCode $0
    ${If} $InstallExitCode == 0
      StrCpy $InstallExitCode 2
    ${EndIf}
    Goto SilentInstallDone
  ${EndIf}
  StrCpy $BrokerStarted 1
SilentInstallPoll:
  Sleep 100
  IntOp $InstallTick $InstallTick + 1
  Call ReadInstallerStatus
  ${If} ${Errors}
    StrCpy $InstallExitCode 13
    Goto SilentInstallDone
  ${EndIf}
  ${If} $InstallTick > 9000
    StrCpy $InstallExitCode 1460
    Goto SilentInstallDone
  ${EndIf}
  ${If} $StatusChanged == 0
    Goto SilentInstallPoll
  ${EndIf}
  ${If} $StatusState == "files_in_use"
    Push "cancel_files"
    Call SendBrokerCommand
    Goto SilentInstallPoll
  ${EndIf}
  ${If} $StatusState == "succeeded"
    ReadINIStr $0 "$StatusPath" "status" "reboot_required"
    ${If} $0 == 1
      SetRebootFlag true
    ${EndIf}
    StrCpy $InstallExitCode 0
    StrCpy $BrokerStarted 0
    Goto SilentInstallDone
  ${EndIf}
  ${If} $StatusState == "failed"
  ${OrIf} $StatusState == "elevation_cancelled"
    StrCpy $InstallExitCode $StatusMsiCode
    ${If} $InstallExitCode == 0
      StrCpy $InstallExitCode $StatusWin32Code
    ${EndIf}
    ${If} $InstallExitCode == 0
      StrCpy $InstallExitCode 1
    ${EndIf}
    StrCpy $BrokerStarted 0
    Goto SilentInstallDone
  ${EndIf}
  Goto SilentInstallPoll
SilentInstallDone:
  Sleep 50
  ${If} $BrokerStarted == 0
    Call CleanupInstallerSession
  ${EndIf}
  SetErrorLevel $InstallExitCode
FunctionEnd

Section "MediaDrop" MainSection
  IfSilent 0 InteractiveNoop
  Call RunSilentInstall
  Quit
InteractiveNoop:
SectionEnd
