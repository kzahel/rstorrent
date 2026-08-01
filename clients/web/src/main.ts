const parameters = new URLSearchParams(window.location.search);

if (parameters.has("demo")) {
  void import("./inspection/bootstrap").then(({ startDemoInspection }) => {
    startDemoInspection(parameters);
  });
} else {
  void import("./legacy-main");
}
