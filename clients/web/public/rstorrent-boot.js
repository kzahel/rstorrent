(function () {
  "use strict";

  function showFailure() {
    var status = document.getElementById("rstorrent-boot-status");
    if (status !== null) {
      status.setAttribute("role", "alert");
      status.textContent =
        "RSTorrent could not start. Reload this page to fetch the current application files.";
    }
  }

  window.addEventListener("error", showFailure);
  window.addEventListener("unhandledrejection", showFailure);
  window.setTimeout(showFailure, 10000);
})();
