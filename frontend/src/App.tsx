import { BrowserRouter, Routes, Route } from 'react-router-dom'
import { AppShell } from './components/AppShell'
import { Dashboard } from './pages/Dashboard'
import { Models } from './pages/Models'
import { Logs } from './pages/Logs'
import { Settings } from './pages/Settings'

function App() {
  return (
    <BrowserRouter>
      <AppShell>
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/models" element={<Models />} />
          <Route path="/logs" element={<Logs />} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </AppShell>
    </BrowserRouter>
  )
}

export default App
