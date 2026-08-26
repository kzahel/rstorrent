const NATIVE_HOST = "com.jstorrent.rstorrent.native";
const PROTOCOL_VERSION = 1;

function sendNativeOperation(op) {
  const request = {
    id: crypto.randomUUID(),
    protocolVersion: PROTOCOL_VERSION,
    op,
  };

  return new Promise((resolve) => {
    chrome.runtime.sendNativeMessage(NATIVE_HOST, request, (response) => {
      const runtimeError = chrome.runtime.lastError;
      if (runtimeError) {
        resolve({
          ok: false,
          error: {
            code: "native_host_unavailable",
            message:
              "RSTorrent Desktop is unavailable. Install it and open it once to finish setup.",
          },
        });
        return;
      }
      if (!response || response.id !== request.id) {
        resolve({
          ok: false,
          error: {
            code: "invalid_native_response",
            message: "RSTorrent Desktop returned an invalid bootstrap response.",
          },
        });
        return;
      }
      resolve(response);
    });
  });
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (
    !message ||
    message.type !== "nativeBootstrap" ||
    !["hello", "launch"].includes(message.op)
  ) {
    return false;
  }

  sendNativeOperation(message.op).then(sendResponse);
  return true;
});
