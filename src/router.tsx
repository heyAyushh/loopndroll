import { createHashRouter } from "react-router-dom";
import App from "./app";
import { HomeRoute } from "./pages/home";
import { SettingsRoute } from "./pages/settings";

export const router = createHashRouter([
  {
    path: "/",
    element: <App />,
    children: [
      {
        index: true,
        element: <HomeRoute />,
      },
      {
        path: "settings",
        element: <SettingsRoute />,
      },
    ],
  },
]);
