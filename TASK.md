# Documentation task

I want to create a `docs` folder containing two things:

- The data flow diagram for this project.
- The threat model assessment using the STRIDE methodology.

## Guidelines

You will need those docs from stellar to accomplish the task:


Link 1: What is threat modelling ?

https://developers.stellar.org/docs/build/security-docs/threat-modeling/threat-modeling-description

Link 2: Threat modeling how to:

https://developers.stellar.org/docs/build/security-docs/threat-modeling/threat-modeling-how-to

Link 3: STRIDE Threat Model Template

https://developers.stellar.org/docs/build/security-docs/threat-modeling/STRIDE-template

Link 4: Pizza Restaurant Example STRIDE Threat Model

https://developers.stellar.org/docs/build/security-docs/threat-modeling/pizza-restaurant-example

Format your results as .md files on a /docs folder with a clear data flow diagram as well as the threat model assessment.

## Precisions about the project

You can use the backend source code located at `~/projects/lend/lend-api` to understand how the backend signature works.

The fiat invest part on the factory is admin only because we can do off-chain deals with bank transfers and reflect it onchain.
