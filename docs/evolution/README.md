# Nova Photo — evolução local

Publicada no Nova Photo 0.15 (20/10/2026): site, servidor de sessões e biblioteca `nova-live` (`cd3d39c`) no ar. A situação de cada um dos 28 itens está em [execution.md](execution.md). Recursos que dependem de serviços ainda não estão disponíveis na infraestrutura publicada.

## Repositórios

- `nova-apps/repos/novaphoto`: motor, interface nativa e MCP.
- `nova-apps/repos/nova-live`: autenticação offline, cliente de documentos e relay com papéis.
- `company-work-os`: API, banco, BrandHub e abertura de links permanentes.

O Photo usa o `nova-live` fixado na revisão `cd3d39c`. Os outros 12 apps continuam na revisão anterior até serem migrados.

## Build e execução local

Na pasta do Photo:

```sh
cargo build --offline -j 2 -p novaphoto -p novaphoto-cli
target/debug/novaphoto
```

O executável de validação está em `target/debug/novaphoto`; o CLI em `target/debug/novaphoto-cli`. O app instalado em `/Applications` não é substituído. Trata-se de build de desenvolvimento, não de instalador ou release otimizado.

Arquivo → Novo carrossel abre o canvas contínuo. Arquivo → Exportar → Exportar cards e o comando `carousel.divide` expõem os fluxos dedicados. A criação por Pranchetas permanece separada.

Exemplo do motor via CLI/MCP:

```json
{"id":"file.new.carousel","params":{"width":1080,"height":1350,"cards":5,"name":"Campanha","background":"white"}}
```

Para largura não divisível, consultar `carousel.preview` e confirmar `fit: "pad"` antes de dividir. Se já houver fatias, confirmar `replace: true`. A exportação usa `file.export.carousel` com `dir`, `prefix` e `format` (`png` ou `jpeg`). Arquivos existentes causam erro; não são substituídos.

## Agente opcional

Ajuda → Conectar agente está desligado inicialmente. Escolha uma pasta de trabalho, ative a conexão e copie a configuração do cliente. O caminho do CLI pode ser editado se ele não estiver ao lado do executável. A conversa fica no Claude Code ou Codex; o Photo mostra atividade e permite testar/desconectar.

A ponte usa stdio no cliente e TCP de loopback autenticado no app. O token é gerado automaticamente e gravado em arquivo privado; o JSON/TOML contém somente o caminho do token. Desconectar fecha conexões e remove o arquivo. Acesso a arquivos passa pela pasta autorizada, inclusive inserção como nova camada. Não é pedida chave de provedor.

Documentação dos clientes: [Codex MCP](https://developers.openai.com/codex/mcp/) e [Claude Code MCP](https://code.claude.com/docs/en/mcp). Geração de imagens depende das ferramentas e autorizações do agente.

## API e chaves — preparação, sem deploy

A migração `0031_wild_nextwave.sql` (tabelas nativas, aditiva) foi aplicada em produção no deploy de 20/10, depois de um backup do banco.

O emissor usa `NATIVE_OFFLINE_PRIVATE_KEY` (PEM PKCS8 Ed25519, aceita `\n` literais) no ambiente da API, configurada no Coolify. A chave pública correspondente está embutida em `nova_live::offline::NOVA_FOODS_SIGNING_PUBLIC_KEY_HEX` (app e relay); `NOVA_OFFLINE_PUBLIC_KEY_HEX` no build ou `NOVA_DOCUMENT_PUBLIC_KEY_HEX` no relay podem substituí-la. Sem chave válida, as funções assinadas ficam indisponíveis. Nunca colocar chave privada ou credenciais no projeto.

As rotas usam a autenticação nativa existente e a permissão `novaphoto`:

- `/api/native/offline-lease`: autorização offline assinada, até sete dias e limitada pela expiração da sessão.
- `/api/native/documents`: documentos, conteúdo, versões, membros, comentários e bilhetes de sessão.
- `/api/native/preferences/novaphoto`: layouts nomeados, com controle de revisão por usuário/organização.
- `/api/native/library`: catálogo e arquivos do BrandHub existente, respeitando acesso ao cliente.

O armazenamento é privado. URLs R2 assinadas não recebem o Bearer da conta. Revisões concorrentes causam conflito, sem substituição silenciosa. Os snapshots offline ficam em uma fila local privada e durável. Backups locais confirmados são mantidos para recuperação; ainda falta política de retenção/limpeza.

Links permanentes contêm somente o ID estável do documento e exigem autenticação. O relay valida assinatura, ator, organização, papel e expiração; leitores não enviam alterações. Um bilhete emitido pode durar até cinco minutos após revogação. Invalidação ativa imediata ainda está pendente.

## Evidências e limites

[validation/real-psd.json](validation/real-psd.json) registra o teste do `Carrossel Feed.psd` fornecido pela equipe. O original foi lido, seu hash conferido novamente e as saídas foram gravadas em `/tmp/novaphoto-evolution-qa/real-psd/`. Os PSDs e imagens proprietárias não entram no repositório ou overrides.

Os renders antes/depois foram comparados pelo próprio Photo, sem diferença de pixels, com 55 camadas e ICC preservados. A recomposição de três cards reproduziu a referência social sRGB. Isso valida esse arquivo; não substitui uma comparação independente com Photoshop nem uma coleção maior.

[baseline.json](baseline.json) registra a revisão inicial e separa os números históricos. Ainda não há aceite de desempenho, memória ou tamanho: faltam builds equivalentes e medições repetidas. Caneta Windows, HDR, monitores físicos, todos os estados de UI e os grupos profissionais da etapa 4 continuam sujeitos à matriz.


