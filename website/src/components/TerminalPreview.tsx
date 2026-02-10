import { motion } from 'framer-motion'

export default function TerminalPreview() {
  return (
    <div className="relative max-w-4xl mx-auto">
      {/* Glow effect */}
      <div className="absolute -inset-1 bg-gradient-to-r from-primary via-accent-cyan to-accent-purple rounded-xl blur-lg opacity-30" />

      {/* Terminal window */}
      <div className="relative bg-bg-dark border border-zinc-700 rounded-xl overflow-hidden shadow-2xl">
        {/* Title bar */}
        <div className="flex items-center gap-2 px-4 py-3 bg-bg-card border-b border-zinc-800">
          <div className="flex gap-2" aria-hidden="true">
            <div className="w-3 h-3 rounded-full bg-red-500/80" />
            <div className="w-3 h-3 rounded-full bg-yellow-500/80" />
            <div className="w-3 h-3 rounded-full bg-green-500/80" />
          </div>
          <span className="text-xs text-zinc-500 ml-2 font-mono">cokacdir — ~/projects</span>
        </div>

        {/* Terminal content */}
        <div className="p-4 font-mono text-sm">
          {/* Header */}
          <div className="text-center text-accent-cyan mb-2 font-bold">
            COKACDIR v0.4.22
          </div>

          {/* Multi panel */}
          <div className="flex gap-2">
            {/* Panel 1 */}
            <motion.div
              initial={{ opacity: 0, x: -20 }}
              animate={{ opacity: 1, x: 0 }}
              transition={{ delay: 0.8 }}
              className="flex-1 border border-primary rounded p-2 bg-bg-card/50"
            >
              <div className="text-primary text-xs mb-2 border-b border-zinc-700 pb-1">
                ~/projects
              </div>
              <div className="space-y-0.5 text-xs">
                <div className="text-zinc-500">..</div>
                <div className="bg-primary/30 text-white px-1">cokacdir/</div>
                <div className="text-zinc-400">website/</div>
                <div className="text-zinc-400">README.md</div>
                <div className="text-zinc-400">package.json</div>
              </div>
            </motion.div>

            {/* Panel 2 */}
            <motion.div
              initial={{ opacity: 0, x: 20 }}
              animate={{ opacity: 1, x: 0 }}
              transition={{ delay: 0.9 }}
              className="flex-1 border border-zinc-600 rounded p-2 bg-bg-card/50"
            >
              <div className="text-zinc-400 text-xs mb-2 border-b border-zinc-700 pb-1">
                ~/projects/cokacdir
              </div>
              <div className="space-y-0.5 text-xs">
                <div className="text-zinc-500">..</div>
                <div className="text-accent-cyan">src/</div>
                <div className="text-accent-cyan">dist/</div>
                <div className="text-zinc-400">tsconfig.json</div>
                <div className="text-zinc-400">LICENSE</div>
              </div>
            </motion.div>
          </div>

          {/* Status bar */}
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ delay: 1.1 }}
            className="flex justify-center gap-4 mt-3 text-xs border-t border-zinc-700 pt-2"
          >
            <span>
              <span className="text-accent-cyan">3</span>
              <span className="text-zinc-500">d </span>
              <span className="text-accent-cyan">5</span>
              <span className="text-zinc-500">f </span>
              <span className="text-accent-cyan">1.2GB</span>
            </span>
            <span className="text-zinc-600">|</span>
            <span>
              <span className="text-accent-cyan">500MB</span>
              <span className="text-zinc-500">/</span>
              <span className="text-accent-cyan">1TB</span>
            </span>
          </motion.div>
        </div>
      </div>
    </div>
  )
}
