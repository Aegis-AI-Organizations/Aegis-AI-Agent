# Agent Hôte Aegis AI

L'Agent Hôte, conçu en Rust, agit au niveau du noyau via eBPF pour intercepter les comportements malveillants. Dans la v2 du MVP, cet agent est configuré dynamiquement par le Brain, tout en envoyant sa télémétrie indépendamment pour éviter la surcharge du Gateway.
