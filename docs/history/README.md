# Histórico preservado

[`CHANGELOG.original.md`](CHANGELOG.original.md) contém o antigo changelog,
sem tradução ou alteração. Seu conteúdo é idêntico ao `CHANGELOG.md` do commit
`bcfe06b8065f37458dbc8beeb37fabf4eba63274`, anterior a esta limpeza.

Os links, plataformas e comandos desse arquivo pertencem ao histórico. Para o
produto atual, use o [README](../../README.md) e o
[changelog ativo](../../CHANGELOG.md).

## Verificação

O verificador exige igualdade byte a byte entre o arquivo arquivado e o commit
de origem. Além disso, compara seções publicadas com suas tags e impede que a
versão do pacote fique abaixo do maior valor entre a tag mais recente e a
versão limite do arquivo histórico, 1.23.0.

A comparação cobre todas as tags por padrão:

```bash
make changelog-check
```

`CHANGELOG_TAGS_TO_CHECK=N` limita uma execução local às N tags mais recentes.
Essa opção não altera a validação do arquivo arquivado nem das reconciliações.

## Reconciliação verificada

A seção 0.6.0 recebeu um item sobre `credentials_path` depois da publicação,
no commit `31a434ae4f626ac3180a469b15c872653e2dc46b`. O arquivo original permanece
intacto. O checker comprova essa inserção comparando os commits, as duas seções
e seus hashes, registrados em [changelog-reconciliations.tsv](changelog-reconciliations.tsv).

As primeiras cinco tags não continham changelog. As tags 0.17.0 e 1.20.0 foram
substituídas pelas respectivas versões de correção. Esses casos estão em
[changelog-unshipped-tags.tsv](changelog-unshipped-tags.tsv), com identidade e
proveniência conferidas a cada execução. Alterar o arquivo arquivado, uma tag,
uma seção ou os dados de reconciliação faz a verificação falhar.
