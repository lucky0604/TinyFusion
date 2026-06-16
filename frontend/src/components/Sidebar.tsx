import { type ReactNode } from 'react'
import { Link, useLocation } from 'react-router-dom'
import {
  LayoutDashboard,
  Cpu,
  ScrollText,
  Settings,
} from 'lucide-react'

interface NavItem {
  path: string
  label: string
  icon: ReactNode
}

const navItems: NavItem[] = [
  { path: '/', label: 'Dashboard', icon: <LayoutDashboard size={16} /> },
  { path: '/models', label: 'Models', icon: <Cpu size={16} /> },
  { path: '/logs', label: 'Logs', icon: <ScrollText size={16} /> },
  { path: '/settings', label: 'Settings', icon: <Settings size={16} /> },
]

export function Sidebar() {
  const location = useLocation()

  return (
    <aside
      className="sidebar flex flex-col h-full bg-[var(--bg-secondary)] border-r border-[var(--border-subtle)] w-[240px] flex-shrink-0"
      role="navigation"
      aria-label="Main navigation"
    >
      {/* Logo */}
      <div className="flex items-center gap-3 px-4" style={{ height: '48px' }}>
        <div className="w-6 h-6 rounded-md bg-[var(--accent-primary)] flex items-center justify-center">
          <Cpu size={14} color="var(--text-inverse)" />
        </div>
        <span className="text-[var(--text-primary)] font-semibold text-sm tracking-tight">
          TinyFusion
        </span>
      </div>

      {/* Nav items */}
      <nav className="flex flex-col gap-1 px-2 mt-2">
        {navItems.map((item) => {
          const isActive = location.pathname === item.path
          return (
            <Link
              key={item.path}
              to={item.path}
              className={`
                flex items-center gap-3 rounded-md cursor-pointer
                transition-colors duration-[150ms] ease-out
                ${isActive
                  ? 'bg-[var(--accent-subtle)]/20 text-[var(--accent-primary)] border-l-2 border-[var(--accent-primary)]'
                  : 'text-[var(--text-secondary)] border-l-2 border-transparent hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]'
                }
              `}
              style={{
                height: '36px',
                paddingLeft: isActive ? '14px' : '16px',
                paddingRight: '16px',
              }}
              aria-current={isActive ? 'page' : undefined}
            >
              {item.icon}
              <span className="text-sm font-normal">{item.label}</span>
            </Link>
          )
        })}
      </nav>

      {/* Spacer */}
      <div className="flex-1" />

      {/* Core status */}
      <div
        className="flex items-center gap-2 px-4 border-t border-[var(--border-subtle)]"
        style={{ height: '48px' }}
        role="status"
        aria-label="Core running"
      >
        <span
          className="w-2 h-2 rounded-full bg-[var(--status-active)]"
          aria-hidden="true"
        />
        <span className="text-xs text-[var(--text-secondary)]">Running</span>
      </div>
    </aside>
  )
}
