import { createBrowserRouter, RouterProvider } from "react-router-dom";
import { AppShell } from "./components/shell";
import { routes } from "./routes/routes";

const router = createBrowserRouter([
  {
    path: "/",
    element: <AppShell />,
    children: routes.map((route) =>
      route.path === "/"
        ? { index: true, element: route.element }
        : { path: route.path.replace(/^\//, ""), element: route.element },
    ),
  },
]);

export function App() {
  return <RouterProvider router={router} />;
}
