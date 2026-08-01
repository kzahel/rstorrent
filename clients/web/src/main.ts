const parameters = new URLSearchParams(window.location.search);

if (parameters.has("demo")) {
  void import("./inspection/bootstrap").then(({ startDemoInspection }) => {
    startDemoInspection(parameters);
  });
} else if (parameters.has("live")) {
  void import("./inspection/bootstrap").then(
    ({ startLiveInspection, renderBootstrapError }) => {
      void startLiveInspection(parameters).catch(renderBootstrapError);
    },
  );
} else {
  void import("./legacy-main");
}
