import assert from "node:assert/strict";
import test from "node:test";

import { clipboardAutofillValue } from "../src/features/clipboard-autofill.js";

test("clipboard autofill accepts a new supported link only when the input is empty", () => {
  const isSupported = (value) => value.includes("youtube.com/");

  assert.equal(
    clipboardAutofillValue({
      clipboardText: " https://www.youtube.com/watch?v=abc123 ",
      inputValue: "",
      lastClipboardText: "",
      isSupported,
    }),
    "https://www.youtube.com/watch?v=abc123"
  );
  assert.equal(
    clipboardAutofillValue({
      clipboardText: "https://www.youtube.com/watch?v=new",
      inputValue: "https://www.instagram.com/p/existing",
      lastClipboardText: "",
      isSupported,
    }),
    ""
  );
  assert.equal(
    clipboardAutofillValue({
      clipboardText: "https://www.youtube.com/watch?v=abc123",
      inputValue: "",
      lastClipboardText: "https://www.youtube.com/watch?v=abc123",
      isSupported,
    }),
    ""
  );
  assert.equal(
    clipboardAutofillValue({
      clipboardText: "not a media link",
      inputValue: "",
      lastClipboardText: "",
      isSupported,
    }),
    ""
  );
});
