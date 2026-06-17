import { useState, useEffect } from 'react'
import { Plus, Trash2, Save, RotateCcw, Check } from 'lucide-react'

interface WorkspaceConfig {
  path: string
  verify_command: string
  verify_timeout_seconds: number
  max_retries: number
}

interface GlobalConfig {
  port: number
  workers: { name: string; endpoint: string; model_id: string }[]
  judge: { name: string; endpoint: string; model_id: string }
  executor: { name: string; endpoint: string; model_id: string }
  workspaces: Record<string, WorkspaceConfig>
  error_keywords: string[]
}

function defaultConfig(): GlobalConfig {
  return {
    port: 9999,
    workers: [{ name: 'Worker 1', endpoint: 'http://localhost:11434/v1', model_id: 'qwen2.5-coder:7b' }],
    judge: { name: 'Judge', endpoint: 'http://localhost:11434/v1', model_id: 'qwen2.5-coder:32b' },
    executor: { name: 'Executor', endpoint: 'http://localhost:11434/v1', model_id: 'deepseek-r1:1.5b' },
    workspaces: {},
    error_keywords: ['stack trace', 'compile error', 'test failed', 'build failed', 'panic'],
  }
}

export function Settings() {
  const [config, setConfig] = useState<GlobalConfig>(defaultConfig)
  const [saved, setSaved] = useState(false)
  const [newWsName, setNewWsName] = useState('')
  const [newWsPath, setNewWsPath] = useState('')
  const [newWsCmd, setNewWsCmd] = useState('cargo build && cargo test')

  useEffect(() => {
    if (typeof window !== 'undefined' && (window as any).__TAURI__) {
      ;(window as any).__TAURI__.invoke('get_config').then((c: GlobalConfig) => {
        if (c) setConfig(c)
      }).catch(() => {})
    }
  }, [])

  const save = async () => {
    if (typeof window !== 'undefined' && (window as any).__TAURI__) {
      try {
        await (window as any).__TAURI__.invoke('save_config', { config })
        await (window as any).__TAURI__.invoke('restart_core')
        setSaved(true)
        setTimeout(() => setSaved(false), 3000)
      } catch (e) {
        alert('Failed to save: ' + e)
      }
    }
  }

  const addWorkspace = () => {
    if (!newWsName.trim()) return
    setConfig({
      ...config,
      workspaces: {
        ...config.workspaces,
        [newWsName]: {
          path: newWsPath || '.',
          verify_command: newWsCmd,
          verify_timeout_seconds: 45,
          max_retries: 3,
        },
      },
    })
    setNewWsName('')
    setNewWsPath('')
  }

  const removeWorkspace = (name: string) => {
    const ws = { ...config.workspaces }
    delete ws[name]
    setConfig({ ...config, workspaces: ws })
  }

  const updateWs = (name: string, field: string, value: string | number) => {
    setConfig({
      ...config,
      workspaces: {
        ...config.workspaces,
        [name]: { ...config.workspaces[name], [field]: value },
      },
    })
  }

  return (
    <div className="p-6 max-w-2xl">
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-xl font-semibold text-[var(--text-primary)] tracking-tight">Settings</h1>
        <button onClick={save} className="flex items-center gap-2 px-4 h-9 rounded-md bg-[var(--accent-primary)] text-white text-sm font-medium cursor-pointer hover:bg-[var(--accent-hover)] transition-colors">
          {saved ? <Check size={16} /> : <Save size={16} />}
          {saved ? 'Saved & Restarted' : 'Save & Restart Core'}
        </button>
      </div>

      <section className="mb-8">
        <h2 className="text-sm font-semibold text-[var(--text-primary)] uppercase tracking-wider mb-3">Gateway</h2>
        <div className="space-y-3">
          <div>
            <label className="block text-xs text-[var(--text-secondary)] mb-1">Port</label>
            <input type="number" value={config.port} onChange={(e) => setConfig({ ...config, port: parseInt(e.target.value) || 9999 })}
              className="w-full h-9 px-3 rounded-md bg-[var(--bg-tertiary)] border border-[var(--border-primary)] text-[var(--text-primary)] text-sm focus:outline-none focus:border-[var(--border-focus)]" />
          </div>
        </div>
      </section>

      <section className="mb-8">
        <h2 className="text-sm font-semibold text-[var(--text-primary)] uppercase tracking-wider mb-3">Model Endpoints</h2>
        {(['judge', 'executor'] as const).map((role) => (
          <div key={role} className="mb-4">
            <h3 className="text-xs font-medium text-[var(--text-secondary)] mb-2 capitalize">{role}</h3>
            {(['endpoint', 'model_id'] as const).map((field) => (
              <div key={field} className="mb-2">
                <label className="block text-xs text-[var(--text-tertiary)] mb-0.5">{field === 'endpoint' ? 'Endpoint URL' : 'Model ID'}</label>
                <input value={config[role][field]} onChange={(e) => {
                  setConfig({ ...config, [role]: { ...config[role], [field]: e.target.value } })
                }} className="w-full h-9 px-3 rounded-md bg-[var(--bg-tertiary)] border border-[var(--border-primary)] text-[var(--text-primary)] text-sm focus:outline-none focus:border-[var(--border-focus)]" />
              </div>
            ))}
          </div>
        ))}

        <h3 className="text-xs font-medium text-[var(--text-secondary)] mb-2">Workers</h3>
        {config.workers.map((w, i) => (
          <div key={i} className="flex gap-2 mb-2">
            <input value={w.endpoint} placeholder="Endpoint" onChange={(e) => {
              const workers = [...config.workers]
              workers[i] = { ...workers[i], endpoint: e.target.value }
              setConfig({ ...config, workers })
            }} className="flex-1 h-9 px-3 rounded-md bg-[var(--bg-tertiary)] border border-[var(--border-primary)] text-[var(--text-primary)] text-sm focus:outline-none focus:border-[var(--border-focus)]" />
            <input value={w.model_id} placeholder="Model ID" onChange={(e) => {
              const workers = [...config.workers]
              workers[i] = { ...workers[i], model_id: e.target.value }
              setConfig({ ...config, workers })
            }} className="w-40 h-9 px-3 rounded-md bg-[var(--bg-tertiary)] border border-[var(--border-primary)] text-[var(--text-primary)] text-sm focus:outline-none focus:border-[var(--border-focus)]" />
            <button onClick={() => {
              setConfig({ ...config, workers: config.workers.filter((_, j) => j !== i) })
            }} className="text-[var(--text-tertiary)] hover:text-[var(--status-error)] cursor-pointer"><Trash2 size={16} /></button>
          </div>
        ))}
        <button
          onClick={() => setConfig({ ...config, workers: [...config.workers, { name: `Worker ${config.workers.length + 1}`, endpoint: 'http://localhost:11434/v1', model_id: '' }] })}
          className="flex items-center gap-1 text-xs text-[var(--accent-primary)] hover:text-[var(--accent-hover)] cursor-pointer mt-1">
          <Plus size={14} /> Add Worker
        </button>
      </section>

      <section className="mb-8">
        <h2 className="text-sm font-semibold text-[var(--text-primary)] uppercase tracking-wider mb-3">Workspaces</h2>
        {Object.entries(config.workspaces).map(([name, ws]) => (
          <div key={name} className="mb-4 p-3 rounded-md bg-[var(--bg-tertiary)] border border-[var(--border-primary)]">
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-medium text-[var(--text-primary)]">{name}</span>
              <button onClick={() => removeWorkspace(name)} className="text-[var(--text-tertiary)] hover:text-[var(--status-error)] cursor-pointer"><Trash2 size={14} /></button>
            </div>
            <div className="grid grid-cols-2 gap-2">
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] mb-0.5">Path</label>
                <input value={ws.path} onChange={(e) => updateWs(name, 'path', e.target.value)}
                  className="w-full h-8 px-2 rounded-md bg-[var(--bg-secondary)] border border-[var(--border-primary)] text-[var(--text-primary)] text-xs focus:outline-none focus:border-[var(--border-focus)]" />
              </div>
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] mb-0.5">Verify Command</label>
                <input value={ws.verify_command} onChange={(e) => updateWs(name, 'verify_command', e.target.value)}
                  className="w-full h-8 px-2 rounded-md bg-[var(--bg-secondary)] border border-[var(--border-primary)] text-[var(--text-primary)] text-xs focus:outline-none focus:border-[var(--border-focus)]" />
              </div>
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] mb-0.5">Timeout (s)</label>
                <input type="number" value={ws.verify_timeout_seconds} onChange={(e) => updateWs(name, 'verify_timeout_seconds', parseInt(e.target.value) || 45)}
                  className="w-full h-8 px-2 rounded-md bg-[var(--bg-secondary)] border border-[var(--border-primary)] text-[var(--text-primary)] text-xs focus:outline-none focus:border-[var(--border-focus)]" />
              </div>
              <div>
                <label className="block text-xs text-[var(--text-tertiary)] mb-0.5">Max Retries</label>
                <input type="number" value={ws.max_retries} onChange={(e) => updateWs(name, 'max_retries', parseInt(e.target.value) || 3)}
                  className="w-full h-8 px-2 rounded-md bg-[var(--bg-secondary)] border border-[var(--border-primary)] text-[var(--text-primary)] text-xs focus:outline-none focus:border-[var(--border-focus)]" />
              </div>
            </div>
          </div>
        ))}
        <div className="flex gap-2">
          <input value={newWsName} onChange={(e) => setNewWsName(e.target.value)} placeholder="Workspace name"
            className="flex-1 h-9 px-3 rounded-md bg-[var(--bg-tertiary)] border border-[var(--border-primary)] text-[var(--text-primary)] text-sm focus:outline-none focus:border-[var(--border-focus)]" />
          <input value={newWsPath} onChange={(e) => setNewWsPath(e.target.value)} placeholder="Path (default: .)"
            className="flex-1 h-9 px-3 rounded-md bg-[var(--bg-tertiary)] border border-[var(--border-primary)] text-[var(--text-primary)] text-sm focus:outline-none focus:border-[var(--border-focus)]" />
          <button onClick={addWorkspace}
            className="flex items-center gap-1 px-3 h-9 rounded-md bg-[var(--accent-primary)] text-white text-sm font-medium cursor-pointer hover:bg-[var(--accent-hover)] transition-colors">
            <Plus size={16} /> Add
          </button>
        </div>
      </section>

      <section className="mb-8">
        <h2 className="text-sm font-semibold text-[var(--text-primary)] uppercase tracking-wider mb-3">Error Keywords</h2>
        <p className="text-xs text-[var(--text-secondary)] mb-2">Trigger diagnostic phase when these keywords appear in user messages.</p>
        <div className="flex flex-wrap gap-2">
          {config.error_keywords.map((kw, i) => (
            <span key={i} className="flex items-center gap-1 px-2 py-1 rounded-md bg-[var(--bg-tertiary)] border border-[var(--border-primary)] text-xs text-[var(--text-primary)]">
              {kw}
              <button onClick={() => {
                setConfig({ ...config, error_keywords: config.error_keywords.filter((_, j) => j !== i) })
              }} className="text-[var(--text-tertiary)] hover:text-[var(--status-error)] cursor-pointer">
                <Trash2 size={12} />
              </button>
            </span>
          ))}
          <input placeholder="Add keyword..."
            onKeyDown={(e) => {
              if (e.key === 'Enter' && e.currentTarget.value.trim()) {
                setConfig({ ...config, error_keywords: [...config.error_keywords, e.currentTarget.value.trim()] })
                e.currentTarget.value = ''
              }
            }}
            className="h-7 px-2 rounded-md bg-[var(--bg-tertiary)] border border-[var(--border-primary)] text-[var(--text-primary)] text-xs focus:outline-none focus:border-[var(--border-focus)] w-32" />
        </div>
      </section>
    </div>
  )
}
