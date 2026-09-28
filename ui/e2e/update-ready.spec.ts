import { test } from "playwright/test";

test("update ready banner restarts through the documented /updates flow", async () => {
  test.fixme(
    true,
    "TODO(P1-903): enable when serve-updates and the real UpdatesGateway land; assert /updates API + WS update.ready → banner → restart.",
  );
});
