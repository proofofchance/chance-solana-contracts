# Historical source continuity

The consolidated candidate descends from public daily lottery commit `1ca405a77115334aa7ba870bec2ae47f3e9793a2`. Older commits are preserved without rewriting. Reconstruct an old build from its original full commit and original inputs, never from the refactored source merely because its domain has the same name.

The separate public giveaway repository remains at https://github.com/proofofchance/giveaways-solana-contracts, including baseline commit `6898dee9e859a9a346a5c2073d9dfcca31d801cf`. It is not retired by this publication.

The planned survivor name is `proofofchance/chance-solana-contracts`. Until the rename gate passes, candidate records use the repository's actual current name. Existing source-reference commitments must remain unchanged after renaming; future records may use the new canonical URL. Do not reuse the old repository name, which would break GitHub redirects.

Candidate CI records one source/ELF manifest per executable at the exact public commit. Manifests and ELFs are uploaded together with commit and image records. Download and retain them in durable release storage for any actual deployment; temporary CI retention is not permanent release hosting.

No historical deployed hash match or remote-verifier registration is asserted by this document. Existing operational manifests name program addresses but do not establish their source commit, original feature flags or build-image digest. Those bindings require historical verification records and an actual binary comparison before claiming a match or retiring old deployment tooling. Repository reachability and a reproducible new candidate are separate checks.
