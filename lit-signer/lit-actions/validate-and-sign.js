(async () => {
  try {
    // 1. Parse the verdict from Callipsos (handle string or object)
    const parsedVerdict = typeof verdict === 'string'
      ? JSON.parse(verdict)
      : verdict;

    // 2. Check the verdict is an approval
    if (!parsedVerdict || parsedVerdict.decision !== 'approved') {
      Lit.Actions.setResponse({
        response: JSON.stringify({
          ok: false,
          reason: `Verdict decision is "${parsedVerdict?.decision || 'missing'}", not approved`,
        }),
      });
      return;
    }

    // 3. Check the verdict has no failed rules
    const failedRules = (parsedVerdict.results || []).filter(
      (r) => r.outcome !== 'pass'
    );
    if (failedRules.length > 0) {
      Lit.Actions.setResponse({
        response: JSON.stringify({
          ok: false,
          reason: `Verdict has ${failedRules.length} non-passing rules`,
        }),
      });
      return;
    }

    // 4. Convert tx hash hex string to bytes for signing
    const toSign = ethers.utils.arrayify(txHash);

    // 5. Sign with the PKP
    const signature = await LitActions.signEcdsa({
      toSign,
      publicKey,
      sigName: 'transactionSignature',
    });

    // 6. Return success
    Lit.Actions.setResponse({
      response: JSON.stringify({
        ok: true,
        signature,
        message: 'Transaction signed by Callipsos-gated PKP',
      }),
    });
  } catch (error) {
    Lit.Actions.setResponse({
      response: JSON.stringify({
        ok: false,
        reason: `Lit Action error: ${error.message}`,
      }),
    });
  }
})();