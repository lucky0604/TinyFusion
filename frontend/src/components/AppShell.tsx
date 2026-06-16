import { Sidebar } from './Sidebar'

interface AppShellProps {
  children: React.ReactNode
}

export function AppShell({ children }: AppShellProps) {
  return (
    <div className="flex h-full w-full">
      <Sidebar />
      <main className="flex-1 overflow-auto bg-[var(--bg-primary)]" id="main-content">
        {children}
      </main>
    </div>
  )
}
