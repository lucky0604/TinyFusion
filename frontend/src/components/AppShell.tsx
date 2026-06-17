import { useState } from 'react'
import { Menu, X } from 'lucide-react'
import { Sidebar } from './Sidebar'

export function AppShell({ children }: { children: React.ReactNode }) {
  const [mobileOpen, setMobileOpen] = useState(false)

  return (
    <div className="flex h-full w-full">
      {/* Desktop sidebar */}
      <div className="hidden md:flex h-full">
        <Sidebar />
      </div>

      {/* Mobile hamburger */}
      <div className="md:hidden flex flex-col w-full">
        <div className="flex items-center gap-2 px-4 border-b border-[var(--border-subtle)] bg-[var(--bg-secondary)]" style={{ height: 48, minHeight: 48 }}>
          <button onClick={() => setMobileOpen(!mobileOpen)} className="text-[var(--text-primary)] cursor-pointer" aria-label="Toggle navigation" style={{ background: 'none', border: 'none', padding: 4, display: 'flex', minWidth: 36, minHeight: 36, alignItems: 'center', justifyContent: 'center' }}>
            <Menu size={20} />
          </button>
          <span className="font-semibold text-sm tracking-tight text-[var(--text-primary)]">TinyFusion</span>
        </div>
        <main className="flex-1 overflow-auto bg-[var(--bg-primary)]" id="main-content">
          {children}
        </main>
      </div>

      {/* Desktop content */}
      <main className="hidden md:flex flex-1 overflow-auto bg-[var(--bg-primary)]" id="main-content-d">
        {children}
      </main>

      {/* Mobile overlay */}
      {mobileOpen && (
        <>
          <div className="fixed inset-0 bg-black/50 z-40 md:hidden" onClick={() => setMobileOpen(false)} />
          <div className="fixed left-0 top-0 bottom-0 z-50 w-[240px] animate-fade-in md:hidden">
            <div className="absolute top-0 right-0 p-2" style={{ zIndex: 1 }}>
              <button onClick={() => setMobileOpen(false)} className="text-[var(--text-primary)] cursor-pointer" style={{ background: 'none', border: 'none', padding: 4, display: 'flex' }}>
                <X size={18} />
              </button>
            </div>
            <Sidebar />
          </div>
        </>
      )}
    </div>
  )
}
