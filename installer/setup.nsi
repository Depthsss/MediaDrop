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

!ifndef APP_VERSION
  !define APP_VERSION "1.0.0"
!endif
!ifndef MSI_PATH
  !error "MSI_PATH is required"
!endif
!ifndef ASSET_DIR
  !error "ASSET_DIR is required"
!endif
!ifndef OUTPUT_PATH
  !define OUTPUT_PATH "MediaDrop-Setup-${APP_VERSION}.exe"
!endif
!define EXTENSION_CONNECTED_EVENT "Local\MediaDrop.ExtensionSetup.Connected.v1"

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
!ifdef PREVIEW_MODE
Var SelfTestPath
!endif
Var Dragging

Var MinimizeHit
Var CloseHit
Var StartHit
Var CancelHit
Var SummaryHit
Var FinishHit
Var LogHit
Var GiveUpHit
Var RetryHit
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
!ifndef PREVIEW_MODE
Var ExtensionRoot
Var MediaDropExe
!endif
Var DefaultBrowserId
Var DefaultProgId
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
Var OperaGxExe
Var OperaExe
Var ChromeExe
Var EdgeExe

Var ExtensionLaterHit
Var ExtensionCopyHit
Var ExtensionPrimaryHit
Var ExtensionBrowserHit0
Var ExtensionBrowserHit1
Var ExtensionBrowserHit2
Var ExtensionBrowserHit3
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
Var InstallProcess
Var InstallTick
Var InstallLog
Var PreviewAnimating

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

Function .onInit
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
  StrCpy $InstallProcess 0
  StrCpy $Progress 0
  StrCpy $LastStage -1

!ifdef PREVIEW_MODE
  ${GetParameters} $0
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
FunctionEnd

Function ExtractUiAssets
  InitPluginsDir
  SetOutPath "$PLUGINSDIR\ui"
  File /oname=screen-welcome.bmp "${ASSET_DIR}\screen-welcome.bmp"
  File /oname=screen-installing.bmp "${ASSET_DIR}\screen-installing.bmp"
  File /oname=screen-extension.bmp "${ASSET_DIR}\screen-extension.bmp"
  File /oname=screen-done.bmp "${ASSET_DIR}\screen-done.bmp"
  File /oname=screen-error.bmp "${ASSET_DIR}\screen-error.bmp"
  File /oname=toggle-on.bmp "${ASSET_DIR}\toggle-on.bmp"
  File /oname=toggle-off.bmp "${ASSET_DIR}\toggle-off.bmp"
  File /oname=InstrumentSans-Regular.ttf "${ASSET_DIR}\InstrumentSans-Regular.ttf"
  File /oname=InstrumentSans-Medium.ttf "${ASSET_DIR}\InstrumentSans-Medium.ttf"
  File /oname=InstrumentSans-SemiBold.ttf "${ASSET_DIR}\InstrumentSans-SemiBold.ttf"
  File /oname=InstrumentSans-Bold.ttf "${ASSET_DIR}\InstrumentSans-Bold.ttf"
  SetOutPath "$PLUGINSDIR\ui\logo"
  File "${ASSET_DIR}\logo\*.bmp"
FunctionEnd

Function ExtractMsi
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=MediaDrop.msi "${MSI_PATH}"
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
  !insertmacro HideControl $StartHit
  !insertmacro HideControl $CancelHit
  !insertmacro HideControl $SummaryHit
  !insertmacro HideControl $FinishHit
  !insertmacro HideControl $LogHit
  !insertmacro HideControl $GiveUpHit
  !insertmacro HideControl $RetryHit
  !insertmacro HideControl $WelcomeToggleHit
  !insertmacro HideControl $LaunchToggleHit
  !insertmacro HideControl $ExtensionToggleHit
  !insertmacro HideControl $WelcomeToggle
  !insertmacro HideControl $LaunchToggle
  !insertmacro HideControl $ExtensionToggle
  !insertmacro HideControl $ExtensionLaterHit
  !insertmacro HideControl $ExtensionCopyHit
  !insertmacro HideControl $ExtensionPrimaryHit
  !insertmacro HideControl $ExtensionBrowserHit0
  !insertmacro HideControl $ExtensionBrowserHit1
  !insertmacro HideControl $ExtensionBrowserHit2
  !insertmacro HideControl $ExtensionBrowserHit3
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
  StrCpy $CurrentScreen "welcome"
  Call HideScreenControls
  Push "$PLUGINSDIR\ui\screen-welcome.bmp"
  Call SetBackground
  !insertmacro ShowControl $StartHit
  !insertmacro ShowControl $WelcomeToggleHit
  !insertmacro ShowControl $WelcomeToggle
FunctionEnd

Function ShowInstalling
  StrCpy $CurrentScreen "installing"
  Call HideScreenControls
  Push "$PLUGINSDIR\ui\screen-installing.bmp"
  Call SetBackground
  !insertmacro ShowControl $CancelHit
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
FunctionEnd

Function ResetDetectedBrowsers
  StrCpy $BrowserCount 0
  StrCpy $SelectedBrowserSlot -1
  StrCpy $BrowserId0 ""
  StrCpy $BrowserId1 ""
  StrCpy $BrowserId2 ""
  StrCpy $BrowserId3 ""
FunctionEnd

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
    StrCpy $3 "opera://extensions"
    Call AddDetectedBrowser
  ${ElseIf} $DefaultBrowserId == "opera"
    StrCpy $0 "opera"
    StrCpy $1 "Opera ★"
    StrCpy $2 $OperaExe
    StrCpy $3 "opera://extensions"
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

  IfFileExists "$LOCALAPPDATA\Programs\Opera GX\launcher.exe" 0 opera_gx_fallback
    StrCpy $OperaGxExe "$LOCALAPPDATA\Programs\Opera GX\launcher.exe"
    Goto opera_gx_ready
  opera_gx_fallback:
  IfFileExists "$LOCALAPPDATA\Programs\Opera GX\opera.exe" 0 opera_gx_program_files
    StrCpy $OperaGxExe "$LOCALAPPDATA\Programs\Opera GX\opera.exe"
    Goto opera_gx_ready
  opera_gx_program_files:
  IfFileExists "$PROGRAMFILES64\Opera GX\launcher.exe" 0 opera_gx_program_files_x86
    StrCpy $OperaGxExe "$PROGRAMFILES64\Opera GX\launcher.exe"
    Goto opera_gx_ready
  opera_gx_program_files_x86:
  IfFileExists "$PROGRAMFILES32\Opera GX\launcher.exe" 0 opera_gx_ready
    StrCpy $OperaGxExe "$PROGRAMFILES32\Opera GX\launcher.exe"
  opera_gx_ready:

  IfFileExists "$LOCALAPPDATA\Programs\Opera\launcher.exe" 0 opera_fallback
    StrCpy $OperaExe "$LOCALAPPDATA\Programs\Opera\launcher.exe"
    Goto opera_ready
  opera_fallback:
  IfFileExists "$LOCALAPPDATA\Programs\Opera\opera.exe" 0 opera_program_files
    StrCpy $OperaExe "$LOCALAPPDATA\Programs\Opera\opera.exe"
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
    StrCpy $3 "opera://extensions"
    Call AddDetectedBrowser
  ${EndIf}
  ${If} $DefaultBrowserId != "opera"
    StrCpy $0 "opera"
    StrCpy $1 "Opera"
    StrCpy $2 $OperaExe
    StrCpy $3 "opera://extensions"
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
  ${If} $BrowserCount > 0
    ${NSD_SetText} $ExtensionBrowserHit0 $BrowserLabel0
    !insertmacro ShowControl $ExtensionBrowserHit0
  ${EndIf}
  ${If} $BrowserCount > 1
    ${NSD_SetText} $ExtensionBrowserHit1 $BrowserLabel1
    !insertmacro ShowControl $ExtensionBrowserHit1
  ${EndIf}
  ${If} $BrowserCount > 2
    ${NSD_SetText} $ExtensionBrowserHit2 $BrowserLabel2
    !insertmacro ShowControl $ExtensionBrowserHit2
  ${EndIf}
  ${If} $BrowserCount > 3
    ${NSD_SetText} $ExtensionBrowserHit3 $BrowserLabel3
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
    Return
  ${EndIf}

  EnableWindow $ExtensionPrimaryHit 1
  EnableWindow $ExtensionCopyHit 1
  SetCtlColors $ExtensionPrimaryHit 0x171512 0xF5C75D
  ${If} $ExtensionBrowserOpened == 1
    ${NSD_SetText} $ExtensionStep0 "✓ $SelectedBrowserLabel uzantılar sayfası açıldı."
  ${Else}
    ${NSD_SetText} $ExtensionStep0 "$SelectedBrowserLabel uzantılar sayfasını aç."
  ${EndIf}
  ${If} $SelectedBrowserId == "edge"
    ${NSD_SetText} $ExtensionStep1 "Sol menüdeki Geliştirici modu anahtarını aç."
  ${Else}
    ${NSD_SetText} $ExtensionStep1 "Sağ üstteki Geliştirici modu anahtarını aç."
  ${EndIf}
  ${If} $ExtensionPathCopied == 1
    ${NSD_SetText} $ExtensionStep2 "✓ Paketlenmemiş öğe klasörü panoya kopyalandı."
  ${Else}
    ${NSD_SetText} $ExtensionStep2 "Paketlenmemiş öğe yükle deyip MediaDrop klasörünü seç."
  ${EndIf}

  ${If} $ExtensionConnected == 1
    ${NSD_SetText} $ExtensionStatusTitle "Eklenti bağlı ✓"
    ${NSD_SetText} $ExtensionStatusDetail "$SelectedBrowserLabel ile MediaDrop arasındaki güvenli köprü hazır."
    ${NSD_SetText} $ExtensionPrimaryHit "Devam"
    SetCtlColors $ExtensionStatusDot 0x78E5AF 0x202126
    SetCtlColors $ExtensionStatusTitle 0x78E5AF 0x202126
  ${ElseIf} $ExtensionBrowserOpened == 1
    ${NSD_SetText} $ExtensionStatusTitle "Tarayıcı adımı açık"
    ${NSD_SetText} $ExtensionStatusDetail "Eklentiyi yüklediğinde bağlantı burada otomatik doğrulanacak."
    ${NSD_SetText} $ExtensionPrimaryHit "Uzantıları yeniden aç"
    SetCtlColors $ExtensionStatusDot 0xF5C75D 0x202126
    SetCtlColors $ExtensionStatusTitle 0xF5C75D 0x202126
  ${Else}
    ${NSD_SetText} $ExtensionStatusTitle "Bağlantı bekleniyor"
    ${NSD_SetText} $ExtensionStatusDetail "Kurucu açık kalacak; görünür MediaDrop penceresi açılmayacak."
    ${NSD_SetText} $ExtensionPrimaryHit "Uzantılar sayfasını aç"
    SetCtlColors $ExtensionStatusDot 0xF5C75D 0x202126
    SetCtlColors $ExtensionStatusTitle 0xF6F5F1 0x202126
  ${EndIf}
FunctionEnd

!ifndef PREVIEW_MODE
Function EnsureExtensionConnectionEvent
  ${If} $ExtensionEvent != 0
    System::Call 'kernel32::CloseHandle(p $ExtensionEvent) i .r0'
    StrCpy $ExtensionEvent 0
  ${EndIf}
  System::Call 'kernel32::CreateEventW(p 0, i 1, i 0, w "${EXTENSION_CONNECTED_EVENT}") p .rExtensionEvent'
  ${If} $ExtensionEvent != 0
    System::Call 'kernel32::ResetEvent(p $ExtensionEvent) i .r0'
  ${EndIf}
FunctionEnd
!endif

Function CopyExtensionPath
  ${If} $ExtensionPath == ""
    Return
  ${EndIf}
  System::Call 'user32::OpenClipboard(p $HWNDPARENT) i .r0'
  ${If} $0 == 0
    Return
  ${EndIf}
  System::Call 'user32::EmptyClipboard() i .r0'
  StrLen $1 $ExtensionPath
  IntOp $1 $1 + 1
  IntOp $1 $1 * 2
  System::Call 'kernel32::GlobalAlloc(i 0x42, i $1) p .r2'
  ${If} $2 != 0
    System::Call 'kernel32::GlobalLock(p $2) p .r3'
    System::Call 'kernel32::lstrcpyW(p $3, w "$ExtensionPath") p .r0'
    System::Call 'kernel32::GlobalUnlock(p $2) i .r0'
    System::Call 'user32::SetClipboardData(i 13, p $2) p .r0'
  ${EndIf}
  System::Call 'user32::CloseClipboard() i .r0'
  StrCpy $ExtensionPathCopied 1
FunctionEnd

Function PrepareExtensionSetup
  StrCpy $ExtensionConnected 0
  StrCpy $ExtensionBrowserOpened 0
  StrCpy $ExtensionPathCopied 0
  Call DetectExtensionBrowsers
  ${If} $BrowserCount > 0
    StrCpy $SelectedBrowserSlot 0
  ${EndIf}
!ifdef PREVIEW_MODE
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
  ${NSD_KillTimer} PollInstallation
  StrCpy $CurrentScreen "extension"
  Call HideScreenControls
  Push "$PLUGINSDIR\ui\screen-extension.bmp"
  Call SetBackground
  !insertmacro ShowControl $ExtensionLaterHit
  !insertmacro ShowControl $ExtensionCopyHit
  !insertmacro ShowControl $ExtensionPrimaryHit
  !insertmacro ShowControl $ExtensionStep0
  !insertmacro ShowControl $ExtensionStep1
  !insertmacro ShowControl $ExtensionStep2
  !insertmacro ShowControl $ExtensionStatusDot
  !insertmacro ShowControl $ExtensionStatusTitle
  !insertmacro ShowControl $ExtensionStatusDetail
  Call PrepareExtensionSetup
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
  StrCpy $SelectedBrowserSlot 0
  StrCpy $ExtensionBrowserOpened 0
  Call UpdateExtensionGuide
FunctionEnd

Function SelectExtensionBrowser1
  StrCpy $SelectedBrowserSlot 1
  StrCpy $ExtensionBrowserOpened 0
  Call UpdateExtensionGuide
FunctionEnd

Function SelectExtensionBrowser2
  StrCpy $SelectedBrowserSlot 2
  StrCpy $ExtensionBrowserOpened 0
  Call UpdateExtensionGuide
FunctionEnd

Function SelectExtensionBrowser3
  StrCpy $SelectedBrowserSlot 3
  StrCpy $ExtensionBrowserOpened 0
  Call UpdateExtensionGuide
FunctionEnd

Function OnExtensionCopyPath
  Call CopyExtensionPath
  Call UpdateExtensionGuide
FunctionEnd

Function OnExtensionPrimary
  ${If} $ExtensionConnected == 1
    StrCpy $ExtensionHandled 1
    Call ShowDone
    Return
  ${EndIf}
  Call LoadSelectedBrowser
  ${If} $SelectedBrowserExe == ""
    Return
  ${EndIf}
  Call CopyExtensionPath
  Exec '"$SelectedBrowserExe" "$SelectedBrowserPage"'
  StrCpy $ExtensionBrowserOpened 1
  Call UpdateExtensionGuide
FunctionEnd

Function OnExtensionLater
  ${NSD_KillTimer} PollExtensionConnection
  StrCpy $ExtensionSetup 0
  StrCpy $ExtensionHandled 1
  Call ShowDone
FunctionEnd

Function SyncDoneExtensionToggle
  ${NSD_FreeBitmap} $ExtensionToggleImage
  ${If} $ExtensionSetup == 1
    ${NSD_SetBitmap} $ExtensionToggle "$PLUGINSDIR\ui\toggle-on.bmp" $ExtensionToggleImage
  ${Else}
    ${NSD_SetBitmap} $ExtensionToggle "$PLUGINSDIR\ui\toggle-off.bmp" $ExtensionToggleImage
  ${EndIf}
FunctionEnd

Function ShowDone
  ${NSD_KillTimer} PollInstallation
  ${NSD_KillTimer} PollExtensionConnection
  StrCpy $CurrentScreen "done"
  Call HideScreenControls
  Push "$PLUGINSDIR\ui\screen-done.bmp"
  Call SetBackground
  !insertmacro ShowControl $SummaryHit
  !insertmacro ShowControl $FinishHit
  !insertmacro ShowControl $LaunchToggleHit
  !insertmacro ShowControl $ExtensionToggleHit
  !insertmacro ShowControl $LaunchToggle
  Call SyncDoneExtensionToggle
  !insertmacro ShowControl $ExtensionToggle
FunctionEnd

Function SetErrorCopy
  ${If} $InstallExitCode == 1602
    ${NSD_SetText} $ErrorLead "Kurulum kullanıcı isteğiyle güvenli biçimde durduruldu. İstersen aynı paketten yeniden başlayabilirsin."
    ${NSD_SetText} $ErrorTitle "Kurulum iptal edildi"
    ${NSD_SetText} $ErrorDetail "Windows Installer değişiklikleri geri aldı. Hata kodu: 1602"
  ${ElseIf} $InstallExitCode == 1618
    ${NSD_SetText} $ErrorLead "Windows üzerinde başka bir kurulum devam ediyor. O işlem tamamlandıktan sonra tekrar deneyebilirsin."
    ${NSD_SetText} $ErrorTitle "Başka bir kurulum çalışıyor"
    ${NSD_SetText} $ErrorDetail "Devam eden Windows Installer işlemi MediaDrop kurulumunu bekletiyor. Hata kodu: 1618"
  ${ElseIf} $InstallExitCode == 1603
    ${NSD_SetText} $ErrorLead "MediaDrop dosyalarından biri kullanımda olabilir. Açık MediaDrop pencerelerini kapatıp tekrar deneyebilirsin."
    ${NSD_SetText} $ErrorTitle "Kurulum tamamlanamadı"
    ${NSD_SetText} $ErrorDetail "Kurulum günlüğü ayrıntıları içerir. Windows Installer hata kodu: 1603"
  ${Else}
    ${NSD_SetText} $ErrorLead "Kurulum güvenli biçimde durdu. Günlüğü açabilir veya aynı paketten yeniden deneyebilirsin."
    ${NSD_SetText} $ErrorTitle "Windows Installer işlemi durdu"
    ${NSD_SetText} $ErrorDetail "Windows Installer hata kodu: $InstallExitCode"
  ${EndIf}
FunctionEnd

Function ShowError
  ${NSD_KillTimer} PollInstallation
  StrCpy $CurrentScreen "error"
  Call HideScreenControls
  Push "$PLUGINSDIR\ui\screen-error.bmp"
  Call SetBackground
  Call SetErrorCopy
  !insertmacro ShowControl $LogHit
  !insertmacro ShowControl $GiveUpHit
  !insertmacro ShowControl $RetryHit
  !insertmacro ShowControl $ErrorLead
  !insertmacro ShowControl $ErrorTitle
  !insertmacro ShowControl $ErrorDetail
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

  SetCtlColors $StageDot0 0x55565C transparent
  SetCtlColors $StageDot1 0x55565C transparent
  SetCtlColors $StageDot2 0x55565C transparent
  SetCtlColors $StageDot3 0x55565C transparent
  SetCtlColors $StageName0 0x72726F transparent
  SetCtlColors $StageName1 0x72726F transparent
  SetCtlColors $StageName2 0x72726F transparent
  SetCtlColors $StageName3 0x72726F transparent
  SetCtlColors $StageState0 0x72726F transparent
  SetCtlColors $StageState1 0x72726F transparent
  SetCtlColors $StageState2 0x72726F transparent
  SetCtlColors $StageState3 0x72726F transparent
  ${NSD_SetText} $StageState0 "BEKLİYOR"
  ${NSD_SetText} $StageState1 "BEKLİYOR"
  ${NSD_SetText} $StageState2 "BEKLİYOR"
  ${NSD_SetText} $StageState3 "BEKLİYOR"

  ${If} $0 > 0
    SetCtlColors $StageDot0 0xDFA326 transparent
    SetCtlColors $StageName0 0xCCCBC5 transparent
    SetCtlColors $StageState0 0xCCCBC5 transparent
    ${NSD_SetText} $StageState0 "TAMAM"
  ${EndIf}
  ${If} $0 > 1
    SetCtlColors $StageDot1 0xDFA326 transparent
    SetCtlColors $StageName1 0xCCCBC5 transparent
    SetCtlColors $StageState1 0xCCCBC5 transparent
    ${NSD_SetText} $StageState1 "TAMAM"
  ${EndIf}
  ${If} $0 > 2
    SetCtlColors $StageDot2 0xDFA326 transparent
    SetCtlColors $StageName2 0xCCCBC5 transparent
    SetCtlColors $StageState2 0xCCCBC5 transparent
    ${NSD_SetText} $StageState2 "TAMAM"
  ${EndIf}
  ${If} $0 > 3
    SetCtlColors $StageDot3 0xDFA326 transparent
    SetCtlColors $StageName3 0xCCCBC5 transparent
    SetCtlColors $StageState3 0xCCCBC5 transparent
    ${NSD_SetText} $StageState3 "TAMAM"
  ${EndIf}

  ${If} $0 == 0
    SetCtlColors $StageDot0 0xF5C75D transparent
    SetCtlColors $StageName0 0xCCCBC5 transparent
    SetCtlColors $StageState0 0xCCCBC5 transparent
    ${NSD_SetText} $StageState0 "ŞİMDİ"
    ${NSD_SetText} $ProgressCurrent "Kurulum dosyaları hazırlanıyor"
  ${ElseIf} $0 == 1
    SetCtlColors $StageDot1 0xF5C75D transparent
    SetCtlColors $StageName1 0xCCCBC5 transparent
    SetCtlColors $StageState1 0xCCCBC5 transparent
    ${NSD_SetText} $StageState1 "ŞİMDİ"
    ${NSD_SetText} $ProgressCurrent "MediaDrop bileşenleri yerleştiriliyor"
  ${ElseIf} $0 == 2
    SetCtlColors $StageDot2 0xF5C75D transparent
    SetCtlColors $StageName2 0xCCCBC5 transparent
    SetCtlColors $StageState2 0xCCCBC5 transparent
    ${NSD_SetText} $StageState2 "ŞİMDİ"
    ${NSD_SetText} $ProgressCurrent "Tarayıcı köprüsü bağlanıyor"
  ${ElseIf} $0 == 3
    SetCtlColors $StageDot3 0xF5C75D transparent
    SetCtlColors $StageName3 0xCCCBC5 transparent
    SetCtlColors $StageState3 0xCCCBC5 transparent
    ${NSD_SetText} $StageState3 "ŞİMDİ"
    ${NSD_SetText} $ProgressCurrent "Son kontroller yapılıyor"
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
FunctionEnd

Function StartInstall
  StrCpy $Progress 0
  StrCpy $LastStage -1
  StrCpy $InstallTick 0
  StrCpy $PreviewAnimating 0
  Call ShowInstalling
  Call UpdateProgress
  System::Call 'user32::UpdateWindow(p $HWNDPARENT) i .r0'

!ifdef PREVIEW_MODE
  StrCpy $PreviewAnimating 1
  ${NSD_CreateTimer} PollInstallation 85
  Return
!else
  Call ExtractMsi
  CreateDirectory "$LOCALAPPDATA\MediaDrop\Kurulum Günlükleri"
  StrCpy $InstallLog "$LOCALAPPDATA\MediaDrop\Kurulum Günlükleri\MediaDrop-Setup-${APP_VERSION}.log"
  StrCpy $1 '/i "$PLUGINSDIR\MediaDrop.msi" /qn /norestart /L*v "$InstallLog"'
  System::Alloc 60
  Pop $0
  System::Call '*$0(i 60, i 0x40, p $HWNDPARENT, t "runas", t "$SYSDIR\msiexec.exe", t "$1", t "$PLUGINSDIR", i 0, p 0, p 0, p 0, p 0, i 0, p 0, p 0)'
  System::Call 'shell32::ShellExecuteExW(p $0) i .r2'
  ${If} $2 == 0
    System::Call 'kernel32::GetLastError() i .rInstallExitCode'
    ${If} $InstallExitCode == 1223
      StrCpy $InstallExitCode 1602
    ${EndIf}
    System::Free $0
    Call ShowError
    Return
  ${EndIf}
  System::Call '*$0(&i 56, p .rInstallProcess)'
  System::Free $0
  ${If} $InstallProcess == 0
    StrCpy $InstallExitCode 1603
    Call ShowError
    Return
  ${EndIf}
  ${NSD_CreateTimer} PollInstallation 85
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
  ${If} $InstallProcess == 0
    Return
  ${EndIf}
  System::Call 'kernel32::WaitForSingleObject(p $InstallProcess, i 0) i .r0'
  ${If} $0 == 258
    IntOp $InstallTick $InstallTick + 1
    ${If} $Progress < 28
      IntOp $1 $InstallTick % 2
    ${ElseIf} $Progress < 60
      IntOp $1 $InstallTick % 5
    ${ElseIf} $Progress < 84
      IntOp $1 $InstallTick % 10
    ${Else}
      IntOp $1 $InstallTick % 20
    ${EndIf}
    ${If} $1 == 0
    ${AndIf} $Progress < 92
      IntOp $Progress $Progress + 1
      Call UpdateProgress
    ${EndIf}
    Return
  ${EndIf}

  System::Call 'kernel32::GetExitCodeProcess(p $InstallProcess, *i .rInstallExitCode) i .r1'
  System::Call 'kernel32::CloseHandle(p $InstallProcess) i .r1'
  StrCpy $InstallProcess 0
  ${If} $InstallExitCode == 0
  ${OrIf} $InstallExitCode == 3010
    ${If} $InstallExitCode == 3010
      SetRebootFlag true
    ${EndIf}
    StrCpy $Progress 100
    Call UpdateProgress
    System::Call 'user32::UpdateWindow(p $HWNDPARENT) i .r0'
    Sleep 260
    ${If} $ExtensionSetup == 1
      Call ShowExtensionSetup
    ${Else}
      Call ShowDone
    ${EndIf}
  ${Else}
    Call ShowError
  ${EndIf}
!endif
FunctionEnd

Function StopInstallation
  ${NSD_KillTimer} PollInstallation
!ifdef PREVIEW_MODE
  StrCpy $PreviewAnimating 0
!else
  ${If} $InstallProcess != 0
    System::Call 'kernel32::TerminateProcess(p $InstallProcess, i 1602) i .r0'
    System::Call 'kernel32::WaitForSingleObject(p $InstallProcess, i 5000) i .r0'
    System::Call 'kernel32::CloseHandle(p $InstallProcess) i .r0'
    StrCpy $InstallProcess 0
  ${EndIf}
!endif
FunctionEnd

Function ToggleWelcomeExtension
  IntOp $ExtensionSetup $ExtensionSetup ^ 1
  StrCpy $ExtensionHandled 0
  ${NSD_FreeBitmap} $WelcomeToggleImage
  ${If} $ExtensionSetup == 1
    ${NSD_SetBitmap} $WelcomeToggle "$PLUGINSDIR\ui\toggle-on.bmp" $WelcomeToggleImage
  ${Else}
    ${NSD_SetBitmap} $WelcomeToggle "$PLUGINSDIR\ui\toggle-off.bmp" $WelcomeToggleImage
  ${EndIf}
FunctionEnd

Function ToggleLaunchApp
  IntOp $LaunchApp $LaunchApp ^ 1
  ${NSD_FreeBitmap} $LaunchToggleImage
  ${If} $LaunchApp == 1
    ${NSD_SetBitmap} $LaunchToggle "$PLUGINSDIR\ui\toggle-on.bmp" $LaunchToggleImage
  ${Else}
    ${NSD_SetBitmap} $LaunchToggle "$PLUGINSDIR\ui\toggle-off.bmp" $LaunchToggleImage
  ${EndIf}
FunctionEnd

Function ToggleDoneExtension
  IntOp $ExtensionSetup $ExtensionSetup ^ 1
  StrCpy $ExtensionHandled 0
  Call SyncDoneExtensionToggle
FunctionEnd

Function OnStart
  Call StartInstall
FunctionEnd

Function OnCancel
  Call StopInstallation
  StrCpy $InstallExitCode 1602
  Call ShowError
FunctionEnd

Function OnRetry
  Call StartInstall
FunctionEnd

Function OnGiveUp
  Quit
FunctionEnd

Function OnMinimize
  SendMessage $HWNDPARENT ${WM_SYSCOMMAND} 0xF020 0
FunctionEnd

Function OnClose
  ${If} $CurrentScreen == "installing"
    Call StopInstallation
    Quit
  ${ElseIf} $CurrentScreen == "done"
    Call OnFinish
  ${Else}
    Quit
  ${EndIf}
FunctionEnd

Function OpenInstallLog
  IfFileExists "$InstallLog" 0 +2
    ExecShell "open" "$InstallLog"
FunctionEnd

!ifndef PREVIEW_MODE
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
!ifdef PREVIEW_MODE
  Quit
!else
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
  Quit
!endif
FunctionEnd

Function DragTimer
  ${If} $Dragging == 1
    Return
  ${EndIf}
  System::Call 'user32::GetAsyncKeyState(i 1) i .r0'
  IntOp $0 $0 & 0x8000
  ${If} $0 == 0
    Return
  ${EndIf}
  System::Alloc 8
  Pop $0
  System::Call 'user32::GetCursorPos(p $0) i .r1'
  System::Call 'user32::ScreenToClient(p $HWNDPARENT, p $0) i .r1'
  System::Call '*$0(i .r1, i .r2)'
  System::Free $0
  ${If} $1 >= 0
  ${AndIf} $1 < 1010
  ${AndIf} $2 >= 0
  ${AndIf} $2 < 64
    StrCpy $Dragging 1
    ${NSD_KillTimer} DragTimer
    System::Call 'user32::ReleaseCapture() i .r0'
    SendMessage $HWNDPARENT ${WM_NCLBUTTONDOWN} 2 0
    ${NSD_CreateTimer} DragTimer 25
    StrCpy $Dragging 0
  ${EndIf}
FunctionEnd

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
  ${NSD_SetBitmap} $WelcomeToggle "$PLUGINSDIR\ui\toggle-off.bmp" $WelcomeToggleImage
  ${NSD_CreateBitmap} 1018 377 46 26 ""
  Pop $LaunchToggle
  !insertmacro PrepareOverlay $LaunchToggle
  ${NSD_SetBitmap} $LaunchToggle "$PLUGINSDIR\ui\toggle-on.bmp" $LaunchToggleImage
  ${NSD_CreateBitmap} 1018 436 46 26 ""
  Pop $ExtensionToggle
  !insertmacro PrepareOverlay $ExtensionToggle
  ${NSD_SetBitmap} $ExtensionToggle "$PLUGINSDIR\ui\toggle-off.bmp" $ExtensionToggleImage

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
  SetCtlColors $ProgressNumber 0xF6F5F1 transparent
  SendMessage $ProgressNumber ${WM_SETFONT} $FontPercent 1
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${SS_RIGHT} ${WS_EX_TRANSPARENT} 804 247 260 42 "Kurulum dosyaları hazırlanıyor"
  Pop $ProgressCurrent
  SetCtlColors $ProgressCurrent 0xCCCBC5 transparent
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
  ${NSD_CreateLabel} 514 340 410 28 "Kurulum dosyaları hazırlanıyor"
  Pop $StageName0
  !insertmacro PrepareOverlay $StageName0
  ${NSD_CreateLabel} 514 389 410 28 "MediaDrop bileşenleri yerleştiriliyor"
  Pop $StageName1
  !insertmacro PrepareOverlay $StageName1
  ${NSD_CreateLabel} 514 438 410 28 "Tarayıcı köprüsü bağlanıyor"
  Pop $StageName2
  !insertmacro PrepareOverlay $StageName2
  ${NSD_CreateLabel} 514 487 410 28 "Son kontroller yapılıyor"
  Pop $StageName3
  !insertmacro PrepareOverlay $StageName3
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${SS_RIGHT} ${WS_EX_TRANSPARENT} 970 340 94 28 "BEKLİYOR"
  Pop $StageState0
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${SS_RIGHT} ${WS_EX_TRANSPARENT} 970 389 94 28 "BEKLİYOR"
  Pop $StageState1
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${SS_RIGHT} ${WS_EX_TRANSPARENT} 970 438 94 28 "BEKLİYOR"
  Pop $StageState2
  nsDialogs::CreateControl STATIC ${WS_CHILD}|${WS_VISIBLE}|${SS_RIGHT} ${WS_EX_TRANSPARENT} 970 487 94 28 "BEKLİYOR"
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
  ${NSD_CreateLabel} 526 491 510 19 ""
  Pop $ExtensionStatusTitle
  SetCtlColors $ExtensionStatusTitle 0xF6F5F1 0x202126
  SendMessage $ExtensionStatusTitle ${WM_SETFONT} $FontSemibold 1
  ${NSD_CreateLabel} 498 491 20 20 "●"
  Pop $ExtensionStatusDot
  SetCtlColors $ExtensionStatusDot 0xF5C75D 0x202126
  SendMessage $ExtensionStatusDot ${WM_SETFONT} $FontSmall 1
  ${NSD_CreateLabel} 526 511 510 19 ""
  Pop $ExtensionStatusDetail
  SetCtlColors $ExtensionStatusDetail 0x8F8F8B 0x202126
  SendMessage $ExtensionStatusDetail ${WM_SETFONT} $FontSmall 1

  !insertmacro CreateHit $MinimizeHit 1022 13 38 38 "Küçült" 0x101114 OnMinimize
  !insertmacro CreateHit $CloseHit 1064 13 38 38 "Kapat" 0x101114 OnClose
  !insertmacro CreateHit $StartHit 884 566 180 46 "Kuruluma başla" 0xF5C75D OnStart
  !insertmacro CreateHit $CancelHit 981 566 83 46 "İptal et" 0x25262B OnCancel
  !insertmacro CreateHit $SummaryHit 482 566 126 46 "Kurulum özeti" 0x25262B OpenInstallLog
  !insertmacro CreateHit $FinishHit 884 566 180 46 "Bitir" 0xF5C75D OnFinish
  !insertmacro CreateHit $LogHit 482 566 146 46 "Kurulum günlüğü" 0x25262B OpenInstallLog
  !insertmacro CreateHit $GiveUpHit 789 566 85 46 "Vazgeç" 0x3B2229 OnGiveUp
  !insertmacro CreateHit $RetryHit 884 566 180 46 "Tekrar dene" 0xF5C75D OnRetry
  !insertmacro CreateHit $WelcomeToggleHit 482 487 582 54 "Tarayıcı eklentisini bağla" 0x17181C ToggleWelcomeExtension
  !insertmacro CreateHit $LaunchToggleHit 482 355 582 59 "MediaDrop'u şimdi aç" 0x17181C ToggleLaunchApp
  !insertmacro CreateHit $ExtensionToggleHit 482 414 582 59 "Tarayıcı eklentisini bağla" 0x17181C ToggleDoneExtension
  !insertmacro CreateHit $ExtensionLaterHit 482 566 116 46 "Daha sonra" 0x25262B OnExtensionLater
  !insertmacro CreateHit $ExtensionCopyHit 634 566 186 46 "Yolu kopyala" 0x25262B OnExtensionCopyPath
  !insertmacro CreateTextButton $ExtensionPrimaryHit 834 566 230 46 "Uzantılar sayfasını aç" 0x171512 0xF5C75D OnExtensionPrimary

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
  Call ShowWelcome
!endif

  nsDialogs::Show
FunctionEnd

Function .onGUIEnd
  ${NSD_KillTimer} DragTimer
  ${NSD_KillTimer} PollInstallation
  ${NSD_KillTimer} PollExtensionConnection
  ${NSD_FreeBitmap} $BackgroundImage
  ${NSD_FreeBitmap} $WelcomeToggleImage
  ${NSD_FreeBitmap} $LaunchToggleImage
  ${NSD_FreeBitmap} $ExtensionToggleImage
  ${NSD_FreeBitmap} $LogoImage
  ${If} $ExtensionEvent != 0
    System::Call 'kernel32::CloseHandle(p $ExtensionEvent) i .r0'
    StrCpy $ExtensionEvent 0
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

Section "MediaDrop" MainSection
  IfSilent 0 InteractiveNoop
  Call ExtractMsi
  CreateDirectory "$LOCALAPPDATA\MediaDrop\Kurulum Günlükleri"
  StrCpy $InstallLog "$LOCALAPPDATA\MediaDrop\Kurulum Günlükleri\MediaDrop-Setup-${APP_VERSION}.log"
  ExecShellWait "runas" "$SYSDIR\msiexec.exe" '/i "$PLUGINSDIR\MediaDrop.msi" /qn /norestart /L*v "$InstallLog"' SW_HIDE $InstallExitCode
  ${If} $InstallExitCode == 3010
    SetRebootFlag true
    SetErrorLevel 0
  ${ElseIf} $InstallExitCode != 0
    SetErrorLevel $InstallExitCode
  ${EndIf}
  Quit
InteractiveNoop:
SectionEnd
