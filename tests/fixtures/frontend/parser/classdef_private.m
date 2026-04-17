classdef Vault
properties (Access=private)
secret = 41;
end
methods
function out = reveal(obj)
out = obj.secret;
end
end
methods (Access=private)
function out = hidden(obj)
out = obj.secret + 1;
end
end
methods (Static, Access=private)
function out = code()
out = 7;
end
end
end
