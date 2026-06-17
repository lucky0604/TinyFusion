import { useState } from 'react'
import { ChevronDown, ChevronRight, Clock, Activity, RefreshCw, CheckCircle2, XCircle } from 'lucide-react'

type SessionState = 'Diagnostic' | 'Execution' | 'Verify' | 'Retry' | 'Done' | 'Failed'

interface SessionEntry {
  id: string
  name: string
  state: SessionState
  retryCount: number
  maxRetries: number
  workers: string
  duration: string
  requests: number
  tokens: number
  createdAt: string
}

const STATE_CONFIG: Record<SessionState, { label: string; color: string; icon: React.ReactNode }> = {
  Diagnostic: { label: 'Diagnostic', color: 'var(--status-active)', icon: <Activity size={14} /> },
  Execution: { label: 'Execution', color: 'var(--status-info)', icon: <RefreshCw size={14} /> },
  Verify: { label: 'Verify', color: 'var(--status-warning)', icon: <CheckCircle2 size={14} /> },
  Retry: { label: 'Retry', color: 'var(--status-error)', icon: <RefreshCw size={14} /> },
  Done: { label: 'Done', color: 'var(--text-tertiary)', icon: <CheckCircle2 size={14} /> },
  Failed: { label: 'Failed', color: 'var(--status-error)', icon: <XCircle size={14} /> },
}

const MOCK_SESSIONS: SessionEntry[] = [
  {
    id: 'sess-a3f2e1', name: 'Fix auth null check', state: 'Done', retryCount: 0, maxRetries: 3,
    workers: 'qwen2.5-coder:7b, deepseek-coder:6.7b', duration: '2m 34s', requests: 3, tokens: 4521,
    createdAt: new Date(Date.now() - 3600000).toISOString(),
  },
  {
    id: 'sess-b4d8c2', name: 'Debug test failures', state: 'Retry', retryCount: 2, maxRetries: 3,
    workers: 'qwen2.5-coder:7b, codellama:7b', duration: '5m 12s', requests: 5, tokens: 8732,
    createdAt: new Date(Date.now() - 1800000).toISOString(),
  },
  {
    id: 'sess-c7e3f9', name: 'Refactor database layer', state: 'Execution', retryCount: 0, maxRetries: 3,
    workers: 'deepseek-coder:6.7b', duration: '1m 08s', requests: 2, tokens: 2190,
    createdAt: new Date(Date.now() - 600000).toISOString(),
  },
  {
    id: 'sess-d2a5b0', name: 'Add rate limiter middleware', state: 'Diagnostic', retryCount: 0, maxRetries: 3,
    workers: 'qwen2.5-coder:7b, deepseek-coder:6.7b, codellama:7b', duration: '0m 45s', requests: 1, tokens: 1240,
    createdAt: new Date().toISOString(),
  },
]

export function Sessions() {
  const [expandedId, setExpandedId] = useState<string | null>(null)

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-semibold text-[var(--text-primary)] tracking-tight">Sessions</h1>
          <p className="text-sm text-[var(--text-secondary)] mt-1">Active and recent diagnostic sessions</p>
        </div>
      </div>

      {MOCK_SESSIONS.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-20 text-center">
          <Activity size={48} className="text-[var(--text-tertiary)] mb-4" />
          <h2 className="text-lg font-medium text-[var(--text-primary)] mb-2">No Sessions</h2>
          <p className="text-sm text-[var(--text-secondary)]">Sessions will appear here when you start using the gateway.</p>
          <p className="text-xs text-[var(--text-tertiary)] mt-2">Point your AI agent to <code className="px-1 py-0.5 rounded bg-[var(--bg-tertiary)]">http://localhost:9999/v1/chat/completions</code></p>
        </div>
      ) : (
        <div className="space-y-2">
          {MOCK_SESSIONS.map((session) => {
            const stateCfg = STATE_CONFIG[session.state]
            const isExpanded = expandedId === session.id

            return (
              <div key={session.id} className="rounded-md border border-[var(--border-primary)] bg-[var(--bg-secondary)] overflow-hidden">
                <button
                  onClick={() => setExpandedId(isExpanded ? null : session.id)}
                  className="w-full flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-[var(--bg-hover)] transition-colors text-left"
                >
                  <span className="flex-shrink-0">{isExpanded ? <ChevronDown size={16} className="text-[var(--text-secondary)]" /> : <ChevronRight size={16} className="text-[var(--text-secondary)]" />}</span>
                  <span className="fw-2 h-2 rounded-full flex-shrink-0" style={{ backgroundColor: stateCfg.color }} />
                  <span className="font-medium text-sm text-[var(--text-primary)] flex-1 truncate">{session.name}</span>
                  <span className="text-xs px-2 py-0.5 rounded-full font-medium" style={{ backgroundColor: stateCfg.color + '20', color: stateCfg.color, border: '1px solid ' + stateCfg.color + '40' }}>
                    {session.state === 'Retry' ? `Retry ${session.retryCount}/${session.maxRetries}` : stateCfg.label}
                  </span>
                  <span className="flex items-center gap-1 text-xs text-[var(--text-tertiary)]">
                    <Clock size={12} /> {session.duration}
                  </span>
                </button>

                {isExpanded && (
                  <div className="px-4 pb-4 border-t border-[var(--border-subtle)]">
                    <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mt-3">
                      <div>
                        <span className="block text-xs text-[var(--text-tertiary)]">Session ID</span>
                        <span className="text-sm font-mono text-[var(--text-primary)]">{session.id}</span>
                      </div>
                      <div>
                        <span className="block text-xs text-[var(--text-tertiary)]">Created</span>
                        <span className="text-sm text-[var(--text-primary)]">{new Date(session.createdAt).toLocaleString()}</span>
                      </div>
                      <div>
                        <span className="block text-xs text-[var(--text-tertiary)]">Requests</span>
                        <span className="text-sm text-[var(--text-primary)]">{session.requests}</span>
                      </div>
                      <div>
                        <span className="block text-xs text-[var(--text-tertiary)]">Tokens</span>
                        <span className="text-sm text-[var(--text-primary)]">{session.tokens.toLocaleString()}</span>
                      </div>
                      <div className="col-span-2">
                        <span className="block text-xs text-[var(--text-tertiary)]">Workers</span>
                        <span className="text-sm font-mono text-[var(--text-primary)]">{session.workers}</span>
                      </div>
                      <div className="col-span-2">
                        <span className="block text-xs text-[var(--text-tertiary)]">Retry Policy</span>
                        <span className="text-sm text-[var(--text-primary)]">{session.retryCount}/{session.maxRetries} retries used</span>
                      </div>
                    </div>
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}
