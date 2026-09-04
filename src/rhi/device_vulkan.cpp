#include "rhi/device.h"

#include <SDL3/SDL_log.h>
#include <SDL3/SDL_video.h>
#include <SDL3/SDL_vulkan.h>

#include <vulkan/vulkan.h>

#include <algorithm>
#include <array>
#include <cstring>
#include <limits>
#include <utility>
#include <vector>

namespace voidui::rhi {

namespace {

constexpr std::uint32_t kFramesInFlight = 2;
VkFormat vertex_format(VertexFormat format) {
  switch (format) {
  case VertexFormat::Float:
    return VK_FORMAT_R32_SFLOAT;
  case VertexFormat::Float2:
    return VK_FORMAT_R32G32_SFLOAT;
  case VertexFormat::Float4:
    return VK_FORMAT_R32G32B32A32_SFLOAT;
  case VertexFormat::UByte4Norm:
    return VK_FORMAT_R8G8B8A8_UNORM;
  case VertexFormat::UInt:
    return VK_FORMAT_R32_UINT;
  }
  return VK_FORMAT_UNDEFINED;
}

VkFormat texture_format(TextureFormat format) {
  switch (format) {
  case TextureFormat::R8Unorm:
    return VK_FORMAT_R8_UNORM;
  case TextureFormat::Rgba8Unorm:
    return VK_FORMAT_R8G8B8A8_UNORM;
  case TextureFormat::Rgba32Float:
    return VK_FORMAT_R32G32B32A32_SFLOAT;
  }
  return VK_FORMAT_R8G8B8A8_UNORM;
}

std::uint32_t grown_capacity(std::uint32_t current, std::uint32_t required) {
  return std::max(required, current ? current * 2 : 4096u);
}

std::uint32_t align_up(std::uint32_t value, std::uint32_t alignment) {
  return (value + alignment - 1) & ~(alignment - 1);
}

} // namespace

struct Buffer::Impl {
  VkDevice device = VK_NULL_HANDLE;
  VkBuffer buffers[kFramesInFlight]{};
  VkDeviceMemory memory[kFramesInFlight]{};
  void *mapped[kFramesInFlight]{};
  std::uint32_t capacities[kFramesInFlight]{};
  std::uint32_t slot = 0;
};

struct Texture::Impl {
  VkDevice device = VK_NULL_HANDLE;
  VkDescriptorPool pool = VK_NULL_HANDLE;
  VkImage image = VK_NULL_HANDLE;
  VkDeviceMemory memory = VK_NULL_HANDLE;
  VkImageView view = VK_NULL_HANDLE;
  VkImageLayout layout = VK_IMAGE_LAYOUT_UNDEFINED;
  std::uint32_t bytes_per_texel = 0;
  std::vector<std::pair<VkSampler, VkDescriptorSet>> descriptors;
};

struct Sampler::Impl {
  VkDevice device = VK_NULL_HANDLE;
  VkSampler sampler = VK_NULL_HANDLE;
};

struct Pipeline::Impl {
  VkDevice device = VK_NULL_HANDLE;
  VkPipeline pipeline = VK_NULL_HANDLE;
  VkPipelineLayout layout = VK_NULL_HANDLE;
};

struct Device::Impl {
  struct StagingBuffer {
    VkBuffer buffer = VK_NULL_HANDLE;
    VkDeviceMemory memory = VK_NULL_HANDLE;
    void *mapped = nullptr;
    std::uint32_t capacity = 0;
  };

  struct Frame {
    VkCommandPool command_pool = VK_NULL_HANDLE;
    VkCommandBuffer commands = VK_NULL_HANDLE;
    VkSemaphore image_ready = VK_NULL_HANDLE;
    VkFence fence = VK_NULL_HANDLE;
    std::vector<StagingBuffer> staging;
    std::uint32_t staging_used = 0;
  };

  SDL_Window *window = nullptr;
  VkInstance instance = VK_NULL_HANDLE;
  VkSurfaceKHR surface = VK_NULL_HANDLE;
  VkPhysicalDevice physical_device = VK_NULL_HANDLE;
  VkDevice device = VK_NULL_HANDLE;
  VkQueue queue = VK_NULL_HANDLE;
  std::uint32_t queue_family = 0;
  VkSurfaceFormatKHR surface_format{};
  VkSwapchainKHR swapchain = VK_NULL_HANDLE;
  VkExtent2D extent{};
  std::vector<VkImage> images;
  std::vector<VkImageView> image_views;
  std::vector<VkFramebuffer> framebuffers;
  std::vector<VkFence> image_fences;

  /// Signalled by the submit that renders into an image, waited on by the
  /// present of that same image -- so there is one per swapchain image, not one
  /// per frame slot.
  ///
  /// Indexing these by frame slot is the version of this that does not work: a
  /// three-image swapchain cycles through images faster than two slots cycle
  /// through semaphores, and the frame fence only says the command buffer
  /// finished, never that the present has already consumed the semaphore. The
  /// third frame then re-signals a semaphore a pending present is still waiting
  /// on, which is undefined and is what the validation layers flag.
  std::vector<VkSemaphore> render_done;
  VkRenderPass render_pass = VK_NULL_HANDLE;
  VkDescriptorSetLayout texture_layout = VK_NULL_HANDLE;
  VkDescriptorPool descriptor_pool = VK_NULL_HANDLE;
  Frame frames[kFramesInFlight];
  std::uint32_t frame_index = 0;
  std::uint32_t image_index = 0;
  VkPipelineLayout bound_layout = VK_NULL_HANDLE;
  bool swapchain_dirty = false;

  std::uint32_t memory_type(std::uint32_t bits,
                            VkMemoryPropertyFlags properties) const {
    VkPhysicalDeviceMemoryProperties memory_properties{};
    vkGetPhysicalDeviceMemoryProperties(physical_device, &memory_properties);
    for (std::uint32_t i = 0; i < memory_properties.memoryTypeCount; ++i) {
      if ((bits & (1u << i)) &&
          (memory_properties.memoryTypes[i].propertyFlags & properties) ==
              properties)
        return i;
    }
    return std::numeric_limits<std::uint32_t>::max();
  }

  bool create_buffer(std::uint32_t size, VkBufferUsageFlags usage,
                     VkBuffer &buffer, VkDeviceMemory &memory,
                     void **mapped = nullptr) const {
    const VkBufferCreateInfo buffer_info{VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
                                         nullptr,
                                         0,
                                         size,
                                         usage,
                                         VK_SHARING_MODE_EXCLUSIVE,
                                         0,
                                         nullptr};
    if (vkCreateBuffer(device, &buffer_info, nullptr, &buffer) != VK_SUCCESS)
      return false;

    VkMemoryRequirements requirements{};
    vkGetBufferMemoryRequirements(device, buffer, &requirements);
    const std::uint32_t type = memory_type(
        requirements.memoryTypeBits, VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT |
                                         VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
    if (type == std::numeric_limits<std::uint32_t>::max())
      return false;

    const VkMemoryAllocateInfo allocate_info{
        VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO, nullptr, requirements.size,
        type};
    if (vkAllocateMemory(device, &allocate_info, nullptr, &memory) !=
        VK_SUCCESS)
      return false;
    if (vkBindBufferMemory(device, buffer, memory, 0) != VK_SUCCESS)
      return false;
    if (mapped && vkMapMemory(device, memory, 0, size, 0, mapped) != VK_SUCCESS)
      return false;
    return true;
  }

  VkExtent2D window_extent(const VkSurfaceCapabilitiesKHR &capabilities) const {
    if (capabilities.currentExtent.width !=
        std::numeric_limits<std::uint32_t>::max())
      return capabilities.currentExtent;

    int width = 0;
    int height = 0;
    SDL_GetWindowSizeInPixels(window, &width, &height);
    return {
        std::clamp(static_cast<std::uint32_t>(std::max(width, 1)),
                   capabilities.minImageExtent.width,
                   capabilities.maxImageExtent.width),
        std::clamp(static_cast<std::uint32_t>(std::max(height, 1)),
                   capabilities.minImageExtent.height,
                   capabilities.maxImageExtent.height),
    };
  }

  void destroy_swapchain_images() {
    for (VkFramebuffer framebuffer : framebuffers)
      vkDestroyFramebuffer(device, framebuffer, nullptr);
    for (VkImageView view : image_views)
      vkDestroyImageView(device, view, nullptr);
    for (VkSemaphore semaphore : render_done)
      vkDestroySemaphore(device, semaphore, nullptr);
    framebuffers.clear();
    image_views.clear();
    images.clear();
    image_fences.clear();
    render_done.clear();
  }

  bool create_swapchain() {
    VkSurfaceCapabilitiesKHR capabilities{};
    if (vkGetPhysicalDeviceSurfaceCapabilitiesKHR(physical_device, surface,
                                                  &capabilities) != VK_SUCCESS)
      return false;
    const VkExtent2D new_extent = window_extent(capabilities);

    std::uint32_t image_count = capabilities.minImageCount + 1;
    if (capabilities.maxImageCount != 0)
      image_count = std::min(image_count, capabilities.maxImageCount);

    const VkSwapchainCreateInfoKHR create_info{
        VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR,
        nullptr,
        0,
        surface,
        image_count,
        surface_format.format,
        surface_format.colorSpace,
        new_extent,
        1,
        VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT,
        VK_SHARING_MODE_EXCLUSIVE,
        0,
        nullptr,
        capabilities.currentTransform,
        VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR,
        VK_PRESENT_MODE_FIFO_KHR,
        VK_TRUE,
        swapchain,
    };

    VkSwapchainKHR replacement = VK_NULL_HANDLE;
    if (vkCreateSwapchainKHR(device, &create_info, nullptr, &replacement) !=
        VK_SUCCESS)
      return false;

    destroy_swapchain_images();
    if (swapchain)
      vkDestroySwapchainKHR(device, swapchain, nullptr);
    swapchain = replacement;
    extent = new_extent;

    vkGetSwapchainImagesKHR(device, swapchain, &image_count, nullptr);
    images.resize(image_count);
    vkGetSwapchainImagesKHR(device, swapchain, &image_count, images.data());
    image_views.resize(image_count);
    framebuffers.resize(image_count);
    image_fences.assign(image_count, VK_NULL_HANDLE);
    render_done.assign(image_count, VK_NULL_HANDLE);

    for (std::uint32_t i = 0; i < image_count; ++i) {
      const VkSemaphoreCreateInfo semaphore_info{
          VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO, nullptr, 0};
      if (vkCreateSemaphore(device, &semaphore_info, nullptr,
                            &render_done[i]) != VK_SUCCESS)
        return false;

      const VkImageViewCreateInfo view_info{
          VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
          nullptr,
          0,
          images[i],
          VK_IMAGE_VIEW_TYPE_2D,
          surface_format.format,
          {},
          {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1},
      };
      if (vkCreateImageView(device, &view_info, nullptr, &image_views[i]) !=
          VK_SUCCESS)
        return false;

      const VkFramebufferCreateInfo framebuffer_info{
          VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO,
          nullptr,
          0,
          render_pass,
          1,
          &image_views[i],
          extent.width,
          extent.height,
          1,
      };
      if (vkCreateFramebuffer(device, &framebuffer_info, nullptr,
                              &framebuffers[i]) != VK_SUCCESS)
        return false;
    }
    return true;
  }

  StagingBuffer *staging_buffer(std::uint32_t required) {
    Frame &frame = frames[frame_index];
    if (frame.staging_used == frame.staging.size())
      frame.staging.emplace_back();

    StagingBuffer &staging = frame.staging[frame.staging_used++];
    if (required <= staging.capacity)
      return &staging;

    if (staging.mapped)
      vkUnmapMemory(device, staging.memory);
    if (staging.buffer)
      vkDestroyBuffer(device, staging.buffer, nullptr);
    if (staging.memory)
      vkFreeMemory(device, staging.memory, nullptr);

    const std::uint32_t capacity = grown_capacity(staging.capacity, required);
    staging = {};
    staging.capacity = capacity;
    if (!create_buffer(staging.capacity, VK_BUFFER_USAGE_TRANSFER_SRC_BIT,
                       staging.buffer, staging.memory, &staging.mapped))
      return nullptr;
    return &staging;
  }

  ~Impl() {
    if (!device) {
      if (surface)
        SDL_Vulkan_DestroySurface(instance, surface, nullptr);
      if (instance)
        vkDestroyInstance(instance, nullptr);
      return;
    }

    vkDeviceWaitIdle(device);
    for (Frame &frame : frames) {
      for (StagingBuffer &staging : frame.staging) {
        if (staging.mapped)
          vkUnmapMemory(device, staging.memory);
        if (staging.buffer)
          vkDestroyBuffer(device, staging.buffer, nullptr);
        if (staging.memory)
          vkFreeMemory(device, staging.memory, nullptr);
      }
      if (frame.fence)
        vkDestroyFence(device, frame.fence, nullptr);
      if (frame.image_ready)
        vkDestroySemaphore(device, frame.image_ready, nullptr);
      if (frame.command_pool)
        vkDestroyCommandPool(device, frame.command_pool, nullptr);
    }
    destroy_swapchain_images();
    if (swapchain)
      vkDestroySwapchainKHR(device, swapchain, nullptr);
    if (descriptor_pool)
      vkDestroyDescriptorPool(device, descriptor_pool, nullptr);
    if (texture_layout)
      vkDestroyDescriptorSetLayout(device, texture_layout, nullptr);
    if (render_pass)
      vkDestroyRenderPass(device, render_pass, nullptr);
    vkDestroyDevice(device, nullptr);
    SDL_Vulkan_DestroySurface(instance, surface, nullptr);
    vkDestroyInstance(instance, nullptr);
  }
};

Buffer::Buffer(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
Buffer::~Buffer() {
  for (std::uint32_t i = 0; i < kFramesInFlight; ++i) {
    if (impl_->mapped[i])
      vkUnmapMemory(impl_->device, impl_->memory[i]);
    if (impl_->buffers[i])
      vkDestroyBuffer(impl_->device, impl_->buffers[i], nullptr);
    if (impl_->memory[i])
      vkFreeMemory(impl_->device, impl_->memory[i], nullptr);
  }
}

Texture::Texture(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
Texture::~Texture() {
  for (const auto &[sampler, set] : impl_->descriptors)
    vkFreeDescriptorSets(impl_->device, impl_->pool, 1, &set);
  if (impl_->view)
    vkDestroyImageView(impl_->device, impl_->view, nullptr);
  if (impl_->image)
    vkDestroyImage(impl_->device, impl_->image, nullptr);
  if (impl_->memory)
    vkFreeMemory(impl_->device, impl_->memory, nullptr);
}

Sampler::Sampler(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
Sampler::~Sampler() {
  if (impl_->sampler)
    vkDestroySampler(impl_->device, impl_->sampler, nullptr);
}

Pipeline::Pipeline(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
Pipeline::~Pipeline() {
  if (impl_->pipeline)
    vkDestroyPipeline(impl_->device, impl_->pipeline, nullptr);
  if (impl_->layout)
    vkDestroyPipelineLayout(impl_->device, impl_->layout, nullptr);
}

Device::Device(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}

std::unique_ptr<Device> Device::create(SDL_Window *window) {
  if (!window)
    return nullptr;

  auto impl = std::make_unique<Impl>();
  impl->window = window;

  Uint32 extension_count = 0;
  const char *const *extensions =
      SDL_Vulkan_GetInstanceExtensions(&extension_count);
  if (!extensions) {
    SDL_Log("voidui: Vulkan instance extensions are unavailable: %s",
            SDL_GetError());
    return nullptr;
  }

  const VkApplicationInfo application_info{VK_STRUCTURE_TYPE_APPLICATION_INFO,
                                           nullptr,
                                           "voidui",
                                           1,
                                           "voidui",
                                           1,
                                           VK_API_VERSION_1_1};
  const VkInstanceCreateInfo instance_info{
      VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
      nullptr,
      0,
      &application_info,
      0,
      nullptr,
      extension_count,
      extensions,
  };
  if (vkCreateInstance(&instance_info, nullptr, &impl->instance) !=
      VK_SUCCESS) {
    SDL_Log("voidui: Vulkan instance creation failed");
    return nullptr;
  }
  if (!SDL_Vulkan_CreateSurface(window, impl->instance, nullptr,
                                &impl->surface)) {
    SDL_Log("voidui: Vulkan surface creation failed: %s", SDL_GetError());
    return nullptr;
  }

  std::uint32_t device_count = 0;
  vkEnumeratePhysicalDevices(impl->instance, &device_count, nullptr);
  std::vector<VkPhysicalDevice> devices(device_count);
  vkEnumeratePhysicalDevices(impl->instance, &device_count, devices.data());
  for (VkPhysicalDevice candidate : devices) {
    std::uint32_t family_count = 0;
    vkGetPhysicalDeviceQueueFamilyProperties(candidate, &family_count, nullptr);
    std::vector<VkQueueFamilyProperties> families(family_count);
    vkGetPhysicalDeviceQueueFamilyProperties(candidate, &family_count,
                                             families.data());
    for (std::uint32_t family = 0; family < family_count; ++family) {
      VkBool32 present = VK_FALSE;
      vkGetPhysicalDeviceSurfaceSupportKHR(candidate, family, impl->surface,
                                           &present);
      if ((families[family].queueFlags & VK_QUEUE_GRAPHICS_BIT) && present) {
        impl->physical_device = candidate;
        impl->queue_family = family;
        break;
      }
    }
    if (impl->physical_device)
      break;
  }
  if (!impl->physical_device) {
    SDL_Log("voidui: no Vulkan device can present this window");
    return nullptr;
  }

  const float priority = 1.0f;
  const VkDeviceQueueCreateInfo queue_info{
      VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
      nullptr,
      0,
      impl->queue_family,
      1,
      &priority};
  const char *device_extensions[]{VK_KHR_SWAPCHAIN_EXTENSION_NAME};
  const VkDeviceCreateInfo device_info{
      VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
      nullptr,
      0,
      1,
      &queue_info,
      0,
      nullptr,
      1,
      device_extensions,
      nullptr,
  };
  if (vkCreateDevice(impl->physical_device, &device_info, nullptr,
                     &impl->device) != VK_SUCCESS) {
    SDL_Log("voidui: Vulkan device creation failed");
    return nullptr;
  }
  vkGetDeviceQueue(impl->device, impl->queue_family, 0, &impl->queue);

  std::uint32_t format_count = 0;
  vkGetPhysicalDeviceSurfaceFormatsKHR(impl->physical_device, impl->surface,
                                       &format_count, nullptr);
  std::vector<VkSurfaceFormatKHR> formats(format_count);
  vkGetPhysicalDeviceSurfaceFormatsKHR(impl->physical_device, impl->surface,
                                       &format_count, formats.data());
  impl->surface_format = formats.front();
  for (const VkSurfaceFormatKHR &format : formats) {
    if (format.format == VK_FORMAT_B8G8R8A8_UNORM) {
      impl->surface_format = format;
      break;
    }
  }

  const VkAttachmentDescription color_attachment{
      0,
      impl->surface_format.format,
      VK_SAMPLE_COUNT_1_BIT,
      VK_ATTACHMENT_LOAD_OP_CLEAR,
      VK_ATTACHMENT_STORE_OP_STORE,
      VK_ATTACHMENT_LOAD_OP_DONT_CARE,
      VK_ATTACHMENT_STORE_OP_DONT_CARE,
      VK_IMAGE_LAYOUT_UNDEFINED,
      VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
  };
  const VkAttachmentReference color_reference{
      0, VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL};
  const VkSubpassDescription subpass{0,       VK_PIPELINE_BIND_POINT_GRAPHICS,
                                     0,       nullptr,
                                     1,       &color_reference,
                                     nullptr, nullptr,
                                     0,       nullptr};
  const VkSubpassDependency dependency{
      VK_SUBPASS_EXTERNAL,
      0,
      VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
      VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
      0,
      VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
      0,
  };
  const VkRenderPassCreateInfo render_pass_info{
      VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
      nullptr,
      0,
      1,
      &color_attachment,
      1,
      &subpass,
      1,
      &dependency};
  if (vkCreateRenderPass(impl->device, &render_pass_info, nullptr,
                         &impl->render_pass) != VK_SUCCESS)
    return nullptr;

  const VkDescriptorSetLayoutBinding texture_binding{
      0, VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, 1,
      VK_SHADER_STAGE_FRAGMENT_BIT, nullptr};
  const VkDescriptorSetLayoutCreateInfo layout_info{
      VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO, nullptr, 0, 1,
      &texture_binding};
  if (vkCreateDescriptorSetLayout(impl->device, &layout_info, nullptr,
                                  &impl->texture_layout) != VK_SUCCESS)
    return nullptr;

  const VkDescriptorPoolSize pool_size{
      VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, 1024};
  // Sets are handed back when a texture goes away. Without the free bit they
  // could only be reclaimed by resetting the whole pool, so every image that
  // came and went burnt one permanently and the pool eventually ran dry.
  const VkDescriptorPoolCreateInfo pool_info{
      VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
      nullptr,
      VK_DESCRIPTOR_POOL_CREATE_FREE_DESCRIPTOR_SET_BIT,
      1024,
      1,
      &pool_size};
  if (vkCreateDescriptorPool(impl->device, &pool_info, nullptr,
                             &impl->descriptor_pool) != VK_SUCCESS)
    return nullptr;

  for (Impl::Frame &frame : impl->frames) {
    const VkCommandPoolCreateInfo command_pool_info{
        VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO, nullptr,
        VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT, impl->queue_family};
    if (vkCreateCommandPool(impl->device, &command_pool_info, nullptr,
                            &frame.command_pool) != VK_SUCCESS)
      return nullptr;

    const VkCommandBufferAllocateInfo command_buffer_info{
        VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO, nullptr,
        frame.command_pool, VK_COMMAND_BUFFER_LEVEL_PRIMARY, 1};
    if (vkAllocateCommandBuffers(impl->device, &command_buffer_info,
                                 &frame.commands) != VK_SUCCESS)
      return nullptr;

    const VkSemaphoreCreateInfo semaphore_info{
        VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO, nullptr, 0};
    const VkFenceCreateInfo fence_info{VK_STRUCTURE_TYPE_FENCE_CREATE_INFO,
                                       nullptr, VK_FENCE_CREATE_SIGNALED_BIT};
    if (vkCreateSemaphore(impl->device, &semaphore_info, nullptr,
                          &frame.image_ready) != VK_SUCCESS ||
        vkCreateFence(impl->device, &fence_info, nullptr, &frame.fence) !=
            VK_SUCCESS)
      return nullptr;
  }

  if (!impl->create_swapchain())
    return nullptr;

  SDL_Log("voidui: GPU backend ready, driver Vulkan");
  return std::unique_ptr<Device>(new Device(std::move(impl)));
}

Device::~Device() = default;

std::unique_ptr<Buffer> Device::create_buffer() {
  auto buffer = std::make_unique<Buffer::Impl>();
  buffer->device = impl_->device;
  return std::unique_ptr<Buffer>(new Buffer(std::move(buffer)));
}

std::unique_ptr<Texture> Device::create_texture(std::uint32_t width,
                                                std::uint32_t height,
                                                TextureFormat format) {
  auto texture = std::make_unique<Texture::Impl>();
  texture->device = impl_->device;
  texture->pool = impl_->descriptor_pool;
  texture->bytes_per_texel = texel_size(format);

  const VkImageCreateInfo image_info{
      VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
      nullptr,
      0,
      VK_IMAGE_TYPE_2D,
      texture_format(format),
      {width, height, 1},
      1,
      1,
      VK_SAMPLE_COUNT_1_BIT,
      VK_IMAGE_TILING_OPTIMAL,
      VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_SAMPLED_BIT,
      VK_SHARING_MODE_EXCLUSIVE,
      0,
      nullptr,
      VK_IMAGE_LAYOUT_UNDEFINED,
  };
  if (vkCreateImage(impl_->device, &image_info, nullptr, &texture->image) !=
      VK_SUCCESS)
    return nullptr;

  VkMemoryRequirements requirements{};
  vkGetImageMemoryRequirements(impl_->device, texture->image, &requirements);
  const std::uint32_t memory_type = impl_->memory_type(
      requirements.memoryTypeBits, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT);
  if (memory_type == std::numeric_limits<std::uint32_t>::max())
    return nullptr;
  const VkMemoryAllocateInfo memory_info{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
                                         nullptr, requirements.size,
                                         memory_type};
  if (vkAllocateMemory(impl_->device, &memory_info, nullptr,
                       &texture->memory) != VK_SUCCESS ||
      vkBindImageMemory(impl_->device, texture->image, texture->memory, 0) !=
          VK_SUCCESS)
    return nullptr;

  const VkImageViewCreateInfo view_info{
      VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
      nullptr,
      0,
      texture->image,
      VK_IMAGE_VIEW_TYPE_2D,
      image_info.format,
      {},
      {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1},
  };
  if (vkCreateImageView(impl_->device, &view_info, nullptr, &texture->view) !=
      VK_SUCCESS)
    return nullptr;
  return std::unique_ptr<Texture>(new Texture(std::move(texture)));
}

std::unique_ptr<Sampler> Device::create_sampler(Filter filter) {
  auto sampler = std::make_unique<Sampler::Impl>();
  sampler->device = impl_->device;
  const VkFilter native_filter =
      filter == Filter::Nearest ? VK_FILTER_NEAREST : VK_FILTER_LINEAR;
  const VkSamplerCreateInfo sampler_info{
      VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO,
      nullptr,
      0,
      native_filter,
      native_filter,
      VK_SAMPLER_MIPMAP_MODE_NEAREST,
      VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
      VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
      VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
      0.0f,
      VK_FALSE,
      1.0f,
      VK_FALSE,
      VK_COMPARE_OP_ALWAYS,
      0.0f,
      0.0f,
      VK_BORDER_COLOR_FLOAT_TRANSPARENT_BLACK,
      VK_FALSE,
  };
  if (vkCreateSampler(impl_->device, &sampler_info, nullptr,
                      &sampler->sampler) != VK_SUCCESS)
    return nullptr;
  return std::unique_ptr<Sampler>(new Sampler(std::move(sampler)));
}

std::unique_ptr<Pipeline> Device::create_pipeline(const PipelineDesc &desc) {
  auto pipeline = std::make_unique<Pipeline::Impl>();
  pipeline->device = impl_->device;

  const VkShaderModuleCreateInfo vertex_module_info{
      VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO, nullptr, 0, desc.vertex.size,
      static_cast<const std::uint32_t *>(desc.vertex.data)};
  const VkShaderModuleCreateInfo fragment_module_info{
      VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO, nullptr, 0,
      desc.fragment.size,
      static_cast<const std::uint32_t *>(desc.fragment.data)};
  VkShaderModule vertex_module = VK_NULL_HANDLE;
  VkShaderModule fragment_module = VK_NULL_HANDLE;
  if (vkCreateShaderModule(impl_->device, &vertex_module_info, nullptr,
                           &vertex_module) != VK_SUCCESS ||
      vkCreateShaderModule(impl_->device, &fragment_module_info, nullptr,
                           &fragment_module) != VK_SUCCESS)
    return nullptr;

  const VkPipelineShaderStageCreateInfo stages[]{
      {VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO, nullptr, 0,
       VK_SHADER_STAGE_VERTEX_BIT, vertex_module, desc.vertex.entrypoint,
       nullptr},
      {VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO, nullptr, 0,
       VK_SHADER_STAGE_FRAGMENT_BIT, fragment_module, desc.fragment.entrypoint,
       nullptr},
  };

  const VkVertexInputBindingDescription binding{0, desc.stride,
                                                VK_VERTEX_INPUT_RATE_INSTANCE};
  std::vector<VkVertexInputAttributeDescription> attributes;
  attributes.reserve(desc.attribute_count);
  for (std::uint32_t i = 0; i < desc.attribute_count; ++i) {
    const VertexAttribute &attribute = desc.attributes[i];
    attributes.push_back({attribute.location, 0,
                          vertex_format(attribute.format), attribute.offset});
  }
  const VkPipelineVertexInputStateCreateInfo vertex_input{
      VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
      nullptr,
      0,
      1,
      &binding,
      static_cast<std::uint32_t>(attributes.size()),
      attributes.data(),
  };
  const VkPipelineInputAssemblyStateCreateInfo input_assembly{
      VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO, nullptr, 0,
      VK_PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP, VK_FALSE};
  const VkPipelineViewportStateCreateInfo viewport_state{
      VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO,
      nullptr,
      0,
      1,
      nullptr,
      1,
      nullptr};
  const VkPipelineRasterizationStateCreateInfo rasterization{
      VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO,
      nullptr,
      0,
      VK_FALSE,
      VK_FALSE,
      VK_POLYGON_MODE_FILL,
      VK_CULL_MODE_NONE,
      VK_FRONT_FACE_COUNTER_CLOCKWISE,
      VK_FALSE,
      0.0f,
      0.0f,
      0.0f,
      1.0f,
  };
  const VkPipelineMultisampleStateCreateInfo multisample{
      VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO,
      nullptr,
      0,
      VK_SAMPLE_COUNT_1_BIT,
      VK_FALSE,
      1.0f,
      nullptr,
      VK_FALSE,
      VK_FALSE};
  const VkPipelineColorBlendAttachmentState blend{
      VK_TRUE,
      VK_BLEND_FACTOR_ONE,
      VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA,
      VK_BLEND_OP_ADD,
      VK_BLEND_FACTOR_ONE,
      VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA,
      VK_BLEND_OP_ADD,
      VK_COLOR_COMPONENT_R_BIT | VK_COLOR_COMPONENT_G_BIT |
          VK_COLOR_COMPONENT_B_BIT | VK_COLOR_COMPONENT_A_BIT,
  };
  const VkPipelineColorBlendStateCreateInfo blend_state{
      VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO,
      nullptr,
      0,
      VK_FALSE,
      VK_LOGIC_OP_COPY,
      1,
      &blend,
      {0.0f, 0.0f, 0.0f, 0.0f}};
  const VkDynamicState dynamic_states[]{VK_DYNAMIC_STATE_VIEWPORT,
                                        VK_DYNAMIC_STATE_SCISSOR};
  const VkPipelineDynamicStateCreateInfo dynamic_state{
      VK_STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO, nullptr, 0, 2,
      dynamic_states};

  const VkPushConstantRange push_constants{VK_SHADER_STAGE_VERTEX_BIT |
                                               VK_SHADER_STAGE_FRAGMENT_BIT,
                                           0, desc.uniform_size};
  const VkDescriptorSetLayout *set_layout =
      desc.sampled_texture ? &impl_->texture_layout : nullptr;
  const VkPipelineLayoutCreateInfo pipeline_layout_info{
      VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
      nullptr,
      0,
      desc.sampled_texture ? 1u : 0u,
      set_layout,
      1,
      &push_constants,
  };
  VkResult result = vkCreatePipelineLayout(impl_->device, &pipeline_layout_info,
                                           nullptr, &pipeline->layout);
  if (result == VK_SUCCESS) {
    const VkGraphicsPipelineCreateInfo pipeline_info{
        VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO,
        nullptr,
        0,
        2,
        stages,
        &vertex_input,
        &input_assembly,
        nullptr,
        &viewport_state,
        &rasterization,
        &multisample,
        nullptr,
        &blend_state,
        &dynamic_state,
        pipeline->layout,
        impl_->render_pass,
        0,
        VK_NULL_HANDLE,
        -1,
    };
    result =
        vkCreateGraphicsPipelines(impl_->device, VK_NULL_HANDLE, 1,
                                  &pipeline_info, nullptr, &pipeline->pipeline);
  }
  vkDestroyShaderModule(impl_->device, vertex_module, nullptr);
  vkDestroyShaderModule(impl_->device, fragment_module, nullptr);
  if (result != VK_SUCCESS) {
    SDL_Log("voidui: Vulkan pipeline creation failed (%d)", result);
    return nullptr;
  }
  return std::unique_ptr<Pipeline>(new Pipeline(std::move(pipeline)));
}

bool Device::begin_frame(std::uint32_t &width, std::uint32_t &height) {
  Impl::Frame &frame = impl_->frames[impl_->frame_index];
  vkWaitForFences(impl_->device, 1, &frame.fence, VK_TRUE, UINT64_MAX);

  VkSurfaceCapabilitiesKHR capabilities{};
  vkGetPhysicalDeviceSurfaceCapabilitiesKHR(impl_->physical_device,
                                            impl_->surface, &capabilities);
  const VkExtent2D desired = impl_->window_extent(capabilities);
  if (impl_->swapchain_dirty || desired.width != impl_->extent.width ||
      desired.height != impl_->extent.height) {
    vkDeviceWaitIdle(impl_->device);
    if (!impl_->create_swapchain())
      return false;
    impl_->swapchain_dirty = false;
  }

  VkResult result = vkAcquireNextImageKHR(impl_->device, impl_->swapchain,
                                          UINT64_MAX, frame.image_ready,
                                          VK_NULL_HANDLE, &impl_->image_index);
  if (result == VK_ERROR_OUT_OF_DATE_KHR) {
    vkDeviceWaitIdle(impl_->device);
    if (!impl_->create_swapchain())
      return false;
    result = vkAcquireNextImageKHR(impl_->device, impl_->swapchain, UINT64_MAX,
                                   frame.image_ready, VK_NULL_HANDLE,
                                   &impl_->image_index);
  }
  if (result != VK_SUCCESS && result != VK_SUBOPTIMAL_KHR)
    return false;

  VkFence &image_fence = impl_->image_fences[impl_->image_index];
  if (image_fence)
    vkWaitForFences(impl_->device, 1, &image_fence, VK_TRUE, UINT64_MAX);
  image_fence = frame.fence;

  vkResetCommandPool(impl_->device, frame.command_pool, 0);
  frame.staging_used = 0;
  const VkCommandBufferBeginInfo begin_info{
      VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO, nullptr,
      VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT, nullptr};
  if (vkBeginCommandBuffer(frame.commands, &begin_info) != VK_SUCCESS)
    return false;
  vkResetFences(impl_->device, 1, &frame.fence);

  width = impl_->extent.width;
  height = impl_->extent.height;
  return true;
}

bool Device::write_buffer(Buffer &buffer, const void *data,
                          std::uint32_t size) {
  const std::uint32_t slot = impl_->frame_index;
  if (size > buffer.impl_->capacities[slot]) {
    if (buffer.impl_->mapped[slot])
      vkUnmapMemory(impl_->device, buffer.impl_->memory[slot]);
    if (buffer.impl_->buffers[slot])
      vkDestroyBuffer(impl_->device, buffer.impl_->buffers[slot], nullptr);
    if (buffer.impl_->memory[slot])
      vkFreeMemory(impl_->device, buffer.impl_->memory[slot], nullptr);

    const std::uint32_t capacity =
        grown_capacity(buffer.impl_->capacities[slot], size);
    buffer.impl_->buffers[slot] = VK_NULL_HANDLE;
    buffer.impl_->memory[slot] = VK_NULL_HANDLE;
    buffer.impl_->mapped[slot] = nullptr;
    if (!impl_->create_buffer(capacity, VK_BUFFER_USAGE_VERTEX_BUFFER_BIT,
                              buffer.impl_->buffers[slot],
                              buffer.impl_->memory[slot],
                              &buffer.impl_->mapped[slot]))
      return false;
    buffer.impl_->capacities[slot] = capacity;
  }
  std::memcpy(buffer.impl_->mapped[slot], data, size);
  buffer.impl_->slot = slot;
  return true;
}

bool Device::upload_texture(Texture &texture, const void *data, std::uint32_t,
                            const TextureUpload *uploads,
                            std::uint32_t upload_count) {
  // A buffer-to-image copy offset has to be a multiple of both 4 and the
  // texel size, which is 16 for the float formats.
  const std::uint32_t alignment = std::max(4u, texture.impl_->bytes_per_texel);

  std::uint32_t staging_size = 0;
  for (std::uint32_t i = 0; i < upload_count; ++i)
    staging_size = align_up(staging_size, alignment) +
                   uploads[i].row_pitch * uploads[i].height;

  Impl::StagingBuffer *staging = impl_->staging_buffer(staging_size);
  if (!staging)
    return false;

  const auto *source = static_cast<const std::uint8_t *>(data);
  auto *destination = static_cast<std::uint8_t *>(staging->mapped);
  std::vector<VkBufferImageCopy> copies;
  copies.reserve(upload_count);
  std::uint32_t offset = 0;
  for (std::uint32_t i = 0; i < upload_count; ++i) {
    const TextureUpload &upload = uploads[i];
    offset = align_up(offset, alignment);
    const std::uint32_t bytes = upload.row_pitch * upload.height;
    std::memcpy(destination + offset, source + upload.offset, bytes);
    copies.push_back({offset,
                      upload.row_pitch / texture.impl_->bytes_per_texel,
                      upload.height,
                      {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1},
                      {static_cast<std::int32_t>(upload.x),
                       static_cast<std::int32_t>(upload.y), 0},
                      {upload.width, upload.height, 1}});
    offset += bytes;
  }

  Impl::Frame &frame = impl_->frames[impl_->frame_index];
  const bool initialized =
      texture.impl_->layout == VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
  const VkImageMemoryBarrier to_transfer{
      VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
      nullptr,
      initialized ? VK_ACCESS_SHADER_READ_BIT : 0u,
      VK_ACCESS_TRANSFER_WRITE_BIT,
      texture.impl_->layout,
      VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
      VK_QUEUE_FAMILY_IGNORED,
      VK_QUEUE_FAMILY_IGNORED,
      texture.impl_->image,
      {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1},
  };
  vkCmdPipelineBarrier(frame.commands,
                       initialized ? VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT
                                   : VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                       VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0,
                       nullptr, 1, &to_transfer);
  vkCmdCopyBufferToImage(frame.commands, staging->buffer, texture.impl_->image,
                         VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                         static_cast<std::uint32_t>(copies.size()),
                         copies.data());

  const VkImageMemoryBarrier to_shader{
      VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
      nullptr,
      VK_ACCESS_TRANSFER_WRITE_BIT,
      VK_ACCESS_SHADER_READ_BIT,
      VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
      VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
      VK_QUEUE_FAMILY_IGNORED,
      VK_QUEUE_FAMILY_IGNORED,
      texture.impl_->image,
      {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1},
  };
  vkCmdPipelineBarrier(frame.commands, VK_PIPELINE_STAGE_TRANSFER_BIT,
                       VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT, 0, 0, nullptr, 0,
                       nullptr, 1, &to_shader);
  texture.impl_->layout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
  return true;
}

void Device::begin_render(float red, float green, float blue, float alpha) {
  Impl::Frame &frame = impl_->frames[impl_->frame_index];
  const VkClearValue clear{{{red, green, blue, alpha}}};
  const VkRenderPassBeginInfo begin_info{
      VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
      nullptr,
      impl_->render_pass,
      impl_->framebuffers[impl_->image_index],
      {{0, 0}, impl_->extent},
      1,
      &clear,
  };
  vkCmdBeginRenderPass(frame.commands, &begin_info, VK_SUBPASS_CONTENTS_INLINE);
  const VkViewport viewport{
      0.0f, 0.0f, float(impl_->extent.width), float(impl_->extent.height),
      0.0f, 1.0f};
  vkCmdSetViewport(frame.commands, 0, 1, &viewport);
}

void Device::bind_pipeline(Pipeline &pipeline) {
  Impl::Frame &frame = impl_->frames[impl_->frame_index];
  vkCmdBindPipeline(frame.commands, VK_PIPELINE_BIND_POINT_GRAPHICS,
                    pipeline.impl_->pipeline);
  impl_->bound_layout = pipeline.impl_->layout;
}

void Device::bind_vertex_buffer(Buffer &buffer) {
  Impl::Frame &frame = impl_->frames[impl_->frame_index];
  const VkDeviceSize offset = 0;
  vkCmdBindVertexBuffers(frame.commands, 0, 1,
                         &buffer.impl_->buffers[buffer.impl_->slot], &offset);
}

bool Device::bind_texture(Texture &texture, Sampler &sampler) {
  VkDescriptorSet descriptor = VK_NULL_HANDLE;
  for (const auto &[native_sampler, set] : texture.impl_->descriptors) {
    if (native_sampler == sampler.impl_->sampler) {
      descriptor = set;
      break;
    }
  }
  if (!descriptor) {
    const VkDescriptorSetAllocateInfo allocate_info{
        VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO, nullptr,
        impl_->descriptor_pool, 1, &impl_->texture_layout};
    if (vkAllocateDescriptorSets(impl_->device, &allocate_info, &descriptor) !=
        VK_SUCCESS) {
      SDL_Log("voidui: Vulkan descriptor-set allocation failed");
      return false;
    }

    const VkDescriptorImageInfo image_info{
        sampler.impl_->sampler, texture.impl_->view,
        VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL};
    const VkWriteDescriptorSet write{
        VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
        nullptr,
        descriptor,
        0,
        0,
        1,
        VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
        &image_info,
        nullptr,
        nullptr,
    };
    vkUpdateDescriptorSets(impl_->device, 1, &write, 0, nullptr);
    texture.impl_->descriptors.emplace_back(sampler.impl_->sampler, descriptor);
  }

  Impl::Frame &frame = impl_->frames[impl_->frame_index];
  vkCmdBindDescriptorSets(frame.commands, VK_PIPELINE_BIND_POINT_GRAPHICS,
                          impl_->bound_layout, 0, 1, &descriptor, 0, nullptr);
  return true;
}

void Device::set_uniforms(const void *data, std::uint32_t size) {
  Impl::Frame &frame = impl_->frames[impl_->frame_index];
  vkCmdPushConstants(frame.commands, impl_->bound_layout,
                     VK_SHADER_STAGE_VERTEX_BIT | VK_SHADER_STAGE_FRAGMENT_BIT,
                     0, size, data);
}

void Device::set_scissor(std::uint32_t x, std::uint32_t y, std::uint32_t width,
                         std::uint32_t height) {
  Impl::Frame &frame = impl_->frames[impl_->frame_index];
  const VkRect2D scissor{
      {static_cast<std::int32_t>(x), static_cast<std::int32_t>(y)},
      {width, height}};
  vkCmdSetScissor(frame.commands, 0, 1, &scissor);
}

void Device::draw(std::uint32_t vertex_count, std::uint32_t instance_count,
                  std::uint32_t first_vertex, std::uint32_t first_instance) {
  Impl::Frame &frame = impl_->frames[impl_->frame_index];
  vkCmdDraw(frame.commands, vertex_count, instance_count, first_vertex,
            first_instance);
}

void Device::end_render() {
  vkCmdEndRenderPass(impl_->frames[impl_->frame_index].commands);
}

void Device::end_frame() {
  Impl::Frame &frame = impl_->frames[impl_->frame_index];
  vkEndCommandBuffer(frame.commands);

  const VkPipelineStageFlags wait_stage =
      VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT;
  const VkSubmitInfo submit_info{
      VK_STRUCTURE_TYPE_SUBMIT_INFO,
      nullptr,
      1,
      &frame.image_ready,
      &wait_stage,
      1,
      &frame.commands,
      1,
      &impl_->render_done[impl_->image_index],
  };
  vkQueueSubmit(impl_->queue, 1, &submit_info, frame.fence);

  const VkPresentInfoKHR present_info{VK_STRUCTURE_TYPE_PRESENT_INFO_KHR,
                                      nullptr,
                                      1,
                                      &impl_->render_done[impl_->image_index],
                                      1,
                                      &impl_->swapchain,
                                      &impl_->image_index,
                                      nullptr};
  const VkResult present_result =
      vkQueuePresentKHR(impl_->queue, &present_info);
  impl_->swapchain_dirty = present_result == VK_ERROR_OUT_OF_DATE_KHR ||
                           present_result == VK_SUBOPTIMAL_KHR;
  impl_->frame_index = (impl_->frame_index + 1) % kFramesInFlight;
}

void Device::wait_idle() { vkDeviceWaitIdle(impl_->device); }

const char *Device::driver() const { return "Vulkan"; }

} // namespace voidui::rhi
