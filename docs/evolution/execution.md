# Nova Photo — matriz de execução

Atualizado em 20/10/2026. Base: `6991c0c`, versão instalada 0.14.0. **Entrega local em andamento; o plano completo ainda não está concluído.** Código presente não equivale a aceite visual, hardware ou produção.

| Item | Trabalho | Evidência inicial | Situação / validação |
|---|---|---|---|
| 1 | Conta offline | Confirmado no código: qualquer falha online impedia acesso. | Implementado cliente + emissão Ed25519 de sete dias; testes de assinatura/expiração/revogação. Exige configurar chave no servidor e chave pública no build; sem deploy. |
| 2 | Barra extra | Confirmada no código e captura inicial. | Removida; compartilhar e presença integrados ao cabeçalho. Captura 1024×680 inspecionada. |
| 3 | Propriedades | Layout inicial flutuante nos temas não Pro. | Encaixado por padrão; flutuação explícita preservada. Captura inspecionada. |
| 4 | Barra estreita | Controles não adaptavam ao espaço. | Menu Mais + rolagem horizontal; captura 1024×680. Matriz completa de resoluções pendente. |
| 5 | Sobreposição | Nem todos os relatos reproduzidos. | Novos diálogos limitados à área e com rolagem; docking existente preservado. Auditoria de todos os diálogos pendente. |
| 6 | Marca | Ícone do app e marca repetida. | Uma marca corporativa do acervo existente; proporção e mipmaps preservados. |
| 7 | Desempenho | Números antigos não reproduzidos. | Pendente: baseline completo equivalente e comparação de tamanho/memória/tempos. |
| 8 | Pincel | Não reproduzido em hardware correspondente. | Pendente: medições de latência e teste de caneta Windows. |
| 9 | Preferências | Contagem histórica ainda não reauditada. | Pendente: auditoria por preferência, sem declarar funcionalidades por presença de menu. |
| 10 | Novo documento | Configurações avançadas ausentes no comando/preset. | Pranchetas, perfil e proporção do pixel implementados; três testes de criação/persistência passaram. Proporção em metadado nativo; PSD 1064 pendente. |
| 11 | Presets sociais | Conjunto aprovado ausente. | Seis modelos editáveis, persistência de presets e recentes. |
| 12 | Novo carrossel | Comando dedicado ausente. | Canvas contínuo, guias/fatias, cinco cards iniciais e prévia. Testes sintéticos passaram. |
| 13 | Dividir/exportar | Exportador de fatias existia; fluxo dedicado ausente. | Divisão com ajuste explícito e undo; exportação PNG/JPEG sRGB, progresso/cancelamento e sem sobrescrita. PNG validado com PSD real e recomposição exata. |
| 14 | Melhorias contínuas | Registro unificado ausente. | Esta matriz e lista abaixo; adicionar evidência e teste a cada solicitação. |
| 15 | PSD | Faltava arquivo real da equipe nesta execução. | Carrossel Feed.psd: 55 camadas, ICC e render preservados em PCraft e PSD; três cards recompostos sem diferença. Um caso, sem promessa geral de paridade. |
| 16 | PDF/AVIF/JXL/CR3 | Recursos herdados requerem levantamento por fluxo. | Pendente: compatibilidade efetiva, PDF multipágina e texto editável. |
| 17 | Docking | Docking nos quatro lados e abas já existia. | Janelas nativas de painéis, recuperação de monitor, restauração e layout local; sincronização explícita por conta. Multimonitor físico pendente. |
| 18 | Espaços profissionais | Não reproduzido nesta etapa. | Pendente: espaços dedicados Selecionar e Máscara, amostra, Ponto de Fuga e Desfoque de Caminho. |
| 19 | Texto | Recursos existentes não auditados nesta execução. | Pendente: validar/corrigir texto em caminho e direção de parágrafo. |
| 20 | Ações/lote | Recursos existentes não auditados nesta execução. | Pendente: validar/corrigir ações independentes e lote com subpastas. |
| 21 | Cor e paletas | Painel e gerenciamento de cor já existentes. | Roda HSV opcional e edição numérica/alpha preservadas; cores oficiais via BrandHub autorizado. Importação em massa de paleta e validação visual pendentes. |
| 22 | MCP opcional | stdio e ponte já existentes; fluxo no produto ausente. | Conectar agente, pasta autorizada, token local automático, teste e desconexão. Jornada em clientes Claude Code e Codex ainda em validação. |
| 23 | Imagens do agente | Operação de inserir pela ponte falhou no teste real. | Ponte corrigida para ler somente pela capacidade autorizada e inserir em nova camada. Ferramentas externas dependem do agente; sem chave de provedor no Photo. |
| 24 | Compartilhamento permanente | Sessões temporárias sem ACL persistente. | Código local: proprietário/editor/leitor, convites somente leitura, escopo setor/empresa, links estáveis e seguir câmera. API e relay validam permissões; jornada desktop com duas contas pendente. |
| 25 | Arquivos/comentários/versões | Não havia identidade e persistência compartilhada. | Código local: ID estável, armazenamento privado, revisões CAS, fila offline, conflitos, comentários ancorados, restauração como nova versão. Testes API/relay passaram; ainda não publicado. |
| 26 | Biblioteca da marca | BrandHub existente no WorkFlow.ind. | Catálogo autorizado com cache local, cores, fontes e inserção de elementos. Fontes exibidas/copiadas, sem instalar binários. Painel dedicado pendente. |
| 27 | Vector editável | Saída SVG do Vector disponível no clipboard. | Colar SVG como Smart Object com fonte preservada. Vínculo local já existe no motor; jornada completa de atualização entre apps pendente. |
| 28 | Caneta/HDR | Não testado em equipamentos correspondentes. | Pendente: inclinação/rotação Windows e HDR, com SDR correto. |

## Limites de aceite

- Sem push, deploy, release, instalação definitiva ou migração dos outros 12 apps.
- O banco de testes é descartável (`workflowind_test`); a migração nova não foi aplicada em produção.
- O app e relay precisam da mesma chave pública; a API usa a chave privada somente no ambiente. Sem chave configurada, o acesso offline e os bilhetes permanentes falham de forma fechada.
- Um bilhete ao vivo emitido pode permanecer válido por até cinco minutos após revogação. A API nega novos acessos e renovações imediatamente. Revogação imediata no relay ainda requer invalidação ativa.
- Ainda faltam: baseline completo de desempenho/tamanho, todos os cenários de UI, testes físicos de monitores/caneta/HDR, equivalência PSD independente e jornadas completas dos dois agentes e de duas contas no desktop.
- O crescimento de tamanho ainda não foi aceito: não há pacote anterior e atual equivalentes medidos.

## Mini funcionalidades e problemas encontrados na validação

| Solicitação / descoberta | Situação | Evidência |
|---|---|---|
| Prancheta nova deve selecionar camada editável, preservando o fundo | Corrigido | `document_setup::tests::a_new_artboard_has_an_editable_child_and_the_requested_background` |
| Inserir imagem no Photo aberto pelo MCP deve usar a pasta autorizada | Corrigido; jornada sendo repetida | `app.place` + teste de capacidade de leitura |
| Documento compartilhado deve manter o ID local após snapshot remoto | Corrigido | teste em `live.rs` |
| Perfil salvo não pode reativar autorização offline revogada | Corrigido | teste do SDK sem lease separado |
| Colisão no segundo card não deve deixar o primeiro arquivo exportado | Corrigido | teste de exportação em sequência |
| Reconectar um editor deve atualizar leitores que aguardaram sem coordenador | Corrigido | fila de snapshots no relay |
