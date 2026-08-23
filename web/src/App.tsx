import { Navigate, Route, Routes } from "react-router-dom"
import { Layout } from "./components/layout"
import { AuthGate } from "./components/auth-gate"
import { SourcesPage } from "./pages/sources"
import { SettingsPage } from "./pages/settings"

export default function App() {
  return (
    <AuthGate>
      <Routes>
        <Route element={<Layout />}>
          <Route index element={<Navigate to="/sources" replace />} />
          <Route path="/sources" element={<SourcesPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="*" element={<Navigate to="/sources" replace />} />
        </Route>
      </Routes>
    </AuthGate>
  )
}
