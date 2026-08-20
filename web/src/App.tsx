import { Navigate, Route, Routes } from "react-router-dom"
import { Layout } from "./components/layout"
import { SourcesPage } from "./pages/sources"
import { EventsPage } from "./pages/events"
import { EventDetailPage } from "./pages/event-detail"

export default function App() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<Navigate to="/sources" replace />} />
        <Route path="/sources" element={<SourcesPage />} />
        <Route path="/events" element={<EventsPage />} />
        <Route path="/events/:id" element={<EventDetailPage />} />
        <Route path="*" element={<Navigate to="/sources" replace />} />
      </Route>
    </Routes>
  )
}
