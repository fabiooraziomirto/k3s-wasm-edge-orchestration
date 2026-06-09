/**
 * API utility functions with timeout support
 */

// API base URL - use relative path for production (Nginx reverse proxy)
// For development, use localhost or environment variable
// When dashboard is in Kubernetes (even accessed via port-forward), Nginx proxies /api/ to the API server service
// When dashboard is local (npm start), use localhost:3001
// IMPORTANT: In production build (Kubernetes), always use relative path /api/ regardless of how you access it
// Since the dashboard is always built and served via Nginx in Kubernetes, we always use relative paths
// The only exception is when explicitly set via REACT_APP_API_BASE_URL (for development)
const API_BASE_URL = process.env.REACT_APP_API_BASE_URL || '';

// Default timeout in milliseconds
const DEFAULT_TIMEOUT = 10000; // 10 seconds

/**
 * Create an AbortController with timeout
 * @param {number} timeout - Timeout in milliseconds
 * @returns {Object} - Object with controller and timeoutId
 */
export const createTimeoutController = (timeout = DEFAULT_TIMEOUT) => {
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), timeout);
  
  return { controller, timeoutId };
};

/**
 * Fetch with timeout support
 * @param {string} url - Request URL
 * @param {Object} options - Fetch options
 * @param {number} timeout - Timeout in milliseconds
 * @returns {Promise} - Fetch promise
 */
export const fetchWithTimeout = async (url, options = {}, timeout = DEFAULT_TIMEOUT) => {
  const { controller, timeoutId } = createTimeoutController(timeout);
  
  try {
    const response = await fetch(url, {
      ...options,
      signal: controller.signal,
      // Add keep-alive and connection reuse
      keepalive: true,
      // Add headers for better connection handling
      headers: {
        'Connection': 'keep-alive',
        ...options.headers
      }
    });
    
    clearTimeout(timeoutId);
    return response;
  } catch (error) {
    clearTimeout(timeoutId);
    
    if (error.name === 'AbortError') {
      throw new Error(`Request timeout after ${timeout}ms`);
    }
    
    // Retry once on connection reset
    if (error.message && (error.message.includes('ERR_CONNECTION_RESET') || error.message.includes('Failed to fetch'))) {
      console.warn(`Connection reset for ${url}, retrying once...`);
      await new Promise(resolve => setTimeout(resolve, 500)); // Wait 500ms before retry
      
      try {
        const retryController = new AbortController();
        const retryTimeoutId = setTimeout(() => retryController.abort(), timeout);
        
        const retryResponse = await fetch(url, {
          ...options,
          signal: retryController.signal,
          keepalive: true,
          headers: {
            'Connection': 'keep-alive',
            ...options.headers
          }
        });
        
        clearTimeout(retryTimeoutId);
        return retryResponse;
      } catch (retryError) {
        throw error; // Throw original error if retry also fails
      }
    }
    
    throw error;
  }
};

/**
 * API call with timeout and error handling
 * @param {string} url - Request URL
 * @param {Object} options - Fetch options
 * @param {number} timeout - Timeout in milliseconds
 * @returns {Promise} - API response
 */
export const apiCall = async (url, options = {}, timeout = DEFAULT_TIMEOUT) => {
  try {
    const response = await fetchWithTimeout(url, options, timeout);
    
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    
    return response;
  } catch (error) {
    console.error(`API call failed for ${url}:`, error);
    throw error;
  }
};

/**
 * GET request with timeout
 * @param {string} url - Request URL
 * @param {number} timeout - Timeout in milliseconds
 * @returns {Promise} - JSON response
 */
export const apiGet = async (url, timeout = DEFAULT_TIMEOUT) => {
  // Prepend API base URL if not absolute URL
  const fullUrl = url.startsWith('http') ? url : `${API_BASE_URL}${url}`;
  const response = await apiCall(fullUrl, { method: 'GET' }, timeout);
  return response.json();
};

/**
 * POST request with timeout
 * @param {string} url - Request URL
 * @param {Object} data - Request body
 * @param {number} timeout - Timeout in milliseconds
 * @returns {Promise} - JSON response
 */
export const apiPost = async (url, data, timeout = DEFAULT_TIMEOUT) => {
  // Prepend API base URL if not absolute URL
  const fullUrl = url.startsWith('http') ? url : `${API_BASE_URL}${url}`;
  const response = await apiCall(fullUrl, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(data)
  }, timeout);
  return response.json();
};

/**
 * PUT request with timeout
 * @param {string} url - Request URL
 * @param {Object} data - Request body
 * @param {number} timeout - Timeout in milliseconds
 * @returns {Promise} - JSON response
 */
export const apiPut = async (url, data, timeout = DEFAULT_TIMEOUT) => {
  // Prepend API base URL if not absolute URL
  const fullUrl = url.startsWith('http') ? url : `${API_BASE_URL}${url}`;
  const response = await apiCall(fullUrl, {
    method: 'PUT',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(data)
  }, timeout);
  return response.json();
};

/**
 * DELETE request with timeout
 * @param {string} url - Request URL
 * @param {number} timeout - Timeout in milliseconds
 * @returns {Promise} - JSON response
 */
export const apiDelete = async (url, timeout = DEFAULT_TIMEOUT) => {
  // Prepend API base URL if not absolute URL
  const fullUrl = url.startsWith('http') ? url : `${API_BASE_URL}${url}`;
  const response = await apiCall(fullUrl, { method: 'DELETE' }, timeout);
  return response.json();
};

/**
 * Multiple API calls with timeout
 * @param {Array} requests - Array of request objects
 * @param {number} timeout - Timeout in milliseconds
 * @returns {Promise} - Array of responses
 */
// Rate limiting: execute requests sequentially with small delay to avoid overwhelming the server
export const apiAll = async (requests, timeout = DEFAULT_TIMEOUT) => {
  const results = [];
  
  for (const request of requests) {
    try {
      if (typeof request === 'string') {
        const result = await apiGet(request, timeout);
        results.push(result);
      } else {
        const result = await apiCall(request.url, request.options, timeout);
        results.push(await result.json());
      }
      // Small delay between requests to avoid overwhelming the server
      await new Promise(resolve => setTimeout(resolve, 50));
    } catch (error) {
      console.error(`API call failed for ${typeof request === 'string' ? request : request.url}:`, error);
      // Return empty data structure instead of throwing to prevent all requests from failing
      if (typeof request === 'string') {
        if (request.includes('/devices')) {
          results.push({ devices: [] });
        } else if (request.includes('/applications')) {
          results.push({ applications: [] });
        } else if (request.includes('/gateways')) {
          results.push({ gateways: [] });
        } else {
          results.push({});
        }
      } else {
        results.push({ data: null });
      }
    }
  }
  
  return results;
};

/**
 * Retry API call with exponential backoff
 * @param {Function} apiFunction - API function to retry
 * @param {number} maxRetries - Maximum number of retries
 * @param {number} baseDelay - Base delay in milliseconds
 * @returns {Promise} - API response
 */
export const apiRetry = async (apiFunction, maxRetries = 3, baseDelay = 1000) => {
  let lastError;
  
  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      return await apiFunction();
    } catch (error) {
      lastError = error;
      
      if (attempt === maxRetries) {
        break;
      }
      
      const delay = baseDelay * Math.pow(2, attempt);
      await new Promise(resolve => setTimeout(resolve, delay));
    }
  }
  
  throw lastError;
};
