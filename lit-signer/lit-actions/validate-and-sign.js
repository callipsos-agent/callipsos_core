async function main(params) {
  try {
    const { verdict, txHash, pkpAddress } = params;

    const parsedVerdict = typeof verdict === 'string'
      ? JSON.parse(verdict)
      : verdict;

    if (!parsedVerdict || parsedVerdict.decision !== 'approved') {
      Lit.Actions.setResponse({
        response: JSON.stringify({
          ok: false,
          reason: `Verdict decision is "${parsedVerdict?.decision || 'missing'}", not approved`,
        }),
      });
      return;
    }

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

    const privateKey = await Lit.Actions.getPrivateKey({ pkpId: pkpAddress });
    const signingKey = new ethers.utils.SigningKey(privateKey);
    const digestBytes = ethers.utils.arrayify(txHash);
    const sig = signingKey.signDigest(digestBytes);
    const signature = ethers.utils.joinSignature(sig);

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
}