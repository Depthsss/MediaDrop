export function clipboardAutofillValue({
  clipboardText = "",
  inputValue = "",
  lastClipboardText = "",
  isSupported = () => false,
} = {}) {
  const value = String(clipboardText || "").trim();
  if (!value || String(inputValue || "").trim() || value === lastClipboardText) return "";
  return isSupported(value) ? value : "";
}
