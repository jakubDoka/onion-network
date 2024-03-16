async function sign(payload) {
	console.log('signing', payload);
	payload = JSON.parse(payload);

	const allInjected = await polkadotExtensionDapp.web3Enable('my cool dapp');
	if (allInjected.length === 0) throw 'no extension installed';

	const accounts = await polkadotExtensionDapp.web3Accounts();
	const choosen = accounts.find(({ address }) => address === payload.address);
	if (!choosen) throw `no account found (by address ${payload.address})`;

	const injector = await polkadotExtensionDapp.web3FromSource(choosen.meta.source);
	const signPayload = injector?.signer?.signPayload;
	if (!signPayload) throw 'signatures not supported';

	const { signature } = await signPayload(payload);
	console.log(signature);
	return signature;
}

async function address(name) {
	if (!name) throw 'no name provided';

	const allInjected = await polkadotExtensionDapp.web3Enable('my cool dapp');
	if (allInjected.length === 0) throw 'no extension installed';

	const accounts = await polkadotExtensionDapp.web3Accounts();
	const choosen = accounts.find(({ meta }) => meta.name === name);
	if (!choosen) throw `no account found (by name ${name})`;
	return choosen.address;
}

this.integration = { sign, address };
