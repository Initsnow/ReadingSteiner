import { Navigate, Route, Routes } from "react-router-dom"
import { Layout } from "./components/layout"
import { DashboardPage } from "./pages/dashboard"
import { SourcesPage } from "./pages/sources"
import { EventsPage } from "./pages/events"
import { EventDetailPage } from "./pages/event-detail"

export default function App() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<DashboardPage />} />
        <Route path="/dashboard" element={<DashboardPage />} />
        <Route path="/sources" element={<SourcesPage />} />
        <Route path="/events" element={<EventsPage />} />
        <Route path="/events/:id" element={<EventDetailPage />} />
        <Route path="*" element={<Navigate to="/dashboard" replace />} />
      </Route>
    </Routes>
  )
}
