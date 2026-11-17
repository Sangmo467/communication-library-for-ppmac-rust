function mexFile = build_pmac_mex(profile, libraryDir)
%BUILD_PMAC_MEX Build the synchronous PMAC C++17 MEX gateway.
%   build_pmac_mex() uses <repo>/target/debug.
%   build_pmac_mex('release') uses <repo>/target/release.
%   build_pmac_mex('release', '/absolute/native/library/directory') overrides it.
%
% First build the matching Rust native library, for example:
%   cargo build -p pmac-ffi --locked
%   cargo build -p pmac-ffi --release --locked
% Select a supported MATLAB C++ compiler with: mex -setup C++
% This function does not download or build Rust dependencies.
%
% Windows: Microsoft uses pmac.dll.lib; GNU/MinGW links the DLL's C exports.
% The DLL directory is added to this MATLAB process's PATH before loading.
% Linux: links libpmac.so with an absolute rpath. When distributing binaries,
% configure LD_LIBRARY_PATH before launching MATLAB or rebuild for that path.
% The native library and MATLAB must have the same 64-bit architecture.

if nargin < 1 || isempty(profile), profile = 'debug'; end
profile = validatestring(profile, {'debug', 'release'}, mfilename, 'profile');
exampleDir = fileparts(mfilename('fullpath'));
repoDir = fileparts(fileparts(exampleDir));
if nargin < 2 || isempty(libraryDir)
    if isfolder(fullfile(repoDir, 'native'))
        libraryDir = fullfile(repoDir, 'native'); % 解压 SDK 后直接引用其预编译库。
    else
        libraryDir = fullfile(repoDir, 'target', profile);
    end
end
if ~(ischar(libraryDir) || (isstring(libraryDir) && isscalar(libraryDir)))
    error('pmac:BuildArgument', 'libraryDir must be a character vector or string scalar.');
end
libraryDir = char(libraryDir);
[exists, attributes] = fileattrib(libraryDir);
if ~exists || ~attributes.directory
    error('pmac:BuildMissing', 'Native library directory does not exist: %s', libraryDir);
end
libraryDir = attributes.Name;
compiler = mex.getCompilerConfigurations('C++', 'Selected');
if isempty(compiler)
    error('pmac:BuildCompiler', 'No C++ compiler is selected. Run mex -setup C++ first.');
end
manufacturer = lower(compiler.Manufacturer);
args = {'-R2018a', ['-I' fullfile(repoDir, 'include')], ...
    ['-I' fullfile(repoDir, 'bindings', 'cpp')], ...
    fullfile(exampleDir, 'pmac_mex.cpp'), '-outdir', exampleDir, '-output', 'pmac_mex'};

if ispc
    dll = fullfile(libraryDir, 'pmac.dll');
    requireFile(dll);
    checkWin64(dll);
    if contains(manufacturer, 'microsoft')
        importLibrary = fullfile(libraryDir, 'pmac.dll.lib');
        requireFile(importLibrary);
        args = [args, {'COMPFLAGS=$COMPFLAGS /std:c++17', importLibrary}];
    elseif contains(manufacturer, 'gnu') || contains(lower(compiler.Name), 'mingw')
        args = [args, {'CXXFLAGS=$CXXFLAGS -std=c++17', dll}];
    else
        error('pmac:BuildCompiler', 'Unsupported Windows C++ compiler: %s', compiler.Name);
    end
    setenv('PATH', [libraryDir pathsep getenv('PATH')]);
elseif isunix && ~ismac
    library = fullfile(libraryDir, 'libpmac.so');
    requireFile(library);
    if ~strcmp(computer('arch'), 'glnxa64')
        error('pmac:BuildArchitecture', 'This example requires 64-bit Linux MATLAB and matching Rust target.');
    end
    % MATLAB passes LDFLAGS through the compiler driver; quote paths with spaces.
    rpath = ['LDFLAGS=$LDFLAGS -Wl,-rpath,"' libraryDir '"'];
    args = [args, {'CXXFLAGS=$CXXFLAGS -std=c++17', rpath, ['-L' libraryDir], '-lpmac'}];
elseif ismac
    library = fullfile(libraryDir, 'libpmac.dylib');
    requireFile(library);
    if ~ismember(computer('arch'), {'maci64', 'maca64'})
        error('pmac:BuildArchitecture', 'Use an Intel/Apple Silicon SDK matching the MATLAB process.');
    end
    % dylib 的 install name 为 @rpath/libpmac.dylib；编译器架构必须与 MATLAB 一致。
    rpath = ['LDFLAGS=$LDFLAGS -Wl,-rpath,"' libraryDir '"'];
    args = [args, {'CXXFLAGS=$CXXFLAGS -std=c++17', rpath, ['-L' libraryDir], '-lpmac'}];
else
    error('pmac:BuildPlatform', 'This example supports Windows, Linux and macOS.');
end
mex(args{:});
addpath(exampleDir);
mexFile = fullfile(exampleDir, ['pmac_mex.' mexext]);
fprintf('Built %s\nNative library: %s\n', mexFile, libraryDir);
fprintf('Check loading with: pmac_mex(''version'')\n');
end

function requireFile(path)
if ~isfile(path)
    error('pmac:BuildMissing', 'Missing %s. Build pmac-ffi for the selected profile first.', path);
end
end

function checkWin64(dll)
if ~strcmp(computer('arch'), 'win64')
    error('pmac:BuildArchitecture', 'This example requires 64-bit Windows MATLAB.');
end
file = fopen(dll, 'rb', 'ieee-le');
if file < 0, error('pmac:BuildLibrary', 'Cannot read %s.', dll); end
cleanup = onCleanup(@() fclose(file)); %#ok<NASGU>
fseek(file, 60, 'bof');
header = fread(file, 1, 'uint32=>double');
if isempty(header) || fseek(file, header, 'bof') ~= 0
    error('pmac:BuildLibrary', 'Invalid PE library: %s.', dll);
end
signature = fread(file, 4, 'uint8=>uint8');
machine = fread(file, 1, 'uint16=>double');
if ~isequal(signature(:).', uint8([80 69 0 0])) || ~isequal(machine, 34404)
    error('pmac:BuildArchitecture', 'pmac.dll must be an x86-64 PE DLL for win64 MATLAB.');
end
end
