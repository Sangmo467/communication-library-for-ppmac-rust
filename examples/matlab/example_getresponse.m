function response = example_getresponse(command, pattern)
%EXAMPLE_GETRESPONSE Connect, read one response, and close the native session.
%   Set PMAC_HOST, PMAC_KNOWN_HOSTS, and authentication environment variables.
%   Run build_pmac_mex first. PMAC_USER defaults to root.
%   example_getresponse() reads Sys.ServoTime.
%   example_getresponse('P1', 'variable') extracts the value from a named response.
if nargin < 1, command = 'Sys.ServoTime'; end
if nargin < 2, pattern = ''; end
host = getenv('PMAC_HOST');
knownHosts = getenv('PMAC_KNOWN_HOSTS');
if isempty(host) || isempty(knownHosts)
    error('pmac:Environment', 'Set PMAC_HOST and PMAC_KNOWN_HOSTS before connecting.');
end
username = getenv('PMAC_USER');
if isempty(username), username = 'root'; end
options = struct('host', host, 'username', username, ...
    'knownHosts', knownHosts, 'password', getenv('PMAC_PASSWORD'), ...
    'privateKey', getenv('PMAC_PRIVATE_KEY'), 'passphrase', getenv('PMAC_PASSPHRASE'));
pmac_mex('connect', options);
cleanup = onCleanup(@closeNative); %#ok<NASGU>
try
    raw = pmac_mex('getresponse', command, pattern);
    response = native2unicode(raw, 'UTF-8');
    fprintf('%s\n', response);
catch problem
    fprintf(2, 'GetResponse failed: %s\n', problem.message);
    rethrow(problem); % Preserve controller diagnostics and MEX identifier.
end
end

function closeNative()
try
    pmac_mex('close');
catch problem
    warning('pmac:Cleanup', 'Native cleanup did not finish: %s. Retry pmac_mex(''close'') before clear.', problem.message);
end
end
