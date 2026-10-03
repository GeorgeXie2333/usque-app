-- No plugin downloads: Neovim 0.11+ and the installed Linux language servers.
vim.g.mapleader = " "
vim.opt.number = true
vim.opt.expandtab = true
vim.opt.shiftwidth = 2
vim.opt.tabstop = 2
vim.opt.signcolumn = "yes"
vim.opt.completeopt = { "menu", "menuone", "noselect" }

vim.lsp.config("rust_analyzer", {
  cmd = { "rust-analyzer" },
  filetypes = { "rust" },
  root_markers = { "Cargo.toml", ".git" },
  settings = {
    ["rust-analyzer"] = {
      cargo = { extraArgs = { "--locked" } },
      check = { extraArgs = { "--locked" } },
    },
  },
})
vim.lsp.config("dart", {
  cmd = { "dart", "language-server", "--protocol=lsp" },
  filetypes = { "dart" },
  root_markers = { "pubspec.yaml" },
})
vim.lsp.config("ruff", {
  cmd = { "ruff", "server" },
  filetypes = { "python" },
  root_markers = { "pyproject.toml", ".git" },
})
vim.lsp.enable({ "rust_analyzer", "dart", "ruff" })
vim.api.nvim_create_autocmd("LspAttach", {
  callback = function(event)
    local opts = { buffer = event.buf }
    for key, action in pairs({
      gd = vim.lsp.buf.definition,
      gr = vim.lsp.buf.references,
      K = vim.lsp.buf.hover,
      ["<leader>rn"] = vim.lsp.buf.rename,
      ["<leader>ca"] = vim.lsp.buf.code_action,
      ["<leader>f"] = vim.lsp.buf.format,
      ["<leader>e"] = vim.diagnostic.open_float,
    }) do
      vim.keymap.set("n", key, action, opts)
    end
    local client = vim.lsp.get_client_by_id(event.data.client_id)
    if client and client:supports_method("textDocument/completion") then
      vim.lsp.completion.enable(true, client.id, event.buf, { autotrigger = true })
      vim.keymap.set("i", "<C-Space>", vim.lsp.completion.get, opts)
    end
  end,
})
